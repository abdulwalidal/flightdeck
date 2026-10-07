mod hid;
mod notify;
mod tray;

use std::env;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::{Duration, Instant};

use hid::{open_status_interface, parse_packet, send_bootstrap, HidEvent, PacketEvent};
use ksni::TrayMethods;
use notify::NotificationTracker;
use tokio::sync::mpsc;
use tray::{HeadsetTray, TrayStatus};

fn print_help() {
    println!("HyperX Cloud Flight Battery Monitor (Linux Tray App)");
    println!();
    println!("USAGE:");
    println!("    hyperx-battery [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -d, --debug    Print raw HID packets and diagnostic logs to stderr");
    println!("    -h, --help     Print help information");
}

/// Spawns the background listener thread that handles blocking HID communication.
fn spawn_hid_worker(
    event_tx: mpsc::Sender<HidEvent>,
    refresh_rx: std_mpsc::Receiver<()>,
    debug: bool,
) {
    thread::spawn(move || {
        let mut device = None;
        let mut last_battery_time = Instant::now();
        let mut currently_connected = false;

        loop {
            // Check if user clicked "Refresh" from the tray menu
            if refresh_rx.try_recv().is_ok() {
                if let Some(ref dev) = device {
                    if debug {
                        eprintln!("[DEBUG] Manual refresh requested: re-sending bootstrap packet");
                    }
                    send_bootstrap(dev, debug);
                }
            }

            // If device is not opened, attempt to connect
            if device.is_none() {
                match open_status_interface(debug) {
                    Ok(dev) => {
                        if debug {
                            eprintln!("[DEBUG] Successfully opened status interface. Sending initial bootstrap...");
                        }
                        send_bootstrap(&dev, debug);
                        device = Some(dev);
                        last_battery_time = Instant::now();
                    }
                    Err(_) => {
                        let _ = event_tx.blocking_send(HidEvent::DongleNotFound);
                        thread::sleep(Duration::from_millis(2000));
                        continue;
                    }
                }
            }

            let dev = device.as_mut().unwrap();
            let mut buf = [0u8; 64];

            match dev.read_timeout(&mut buf, 1000) {
                Ok(0) => {
                    // Read timed out (1 second with no packet).
                    // If no battery status packet has been received for > 30 seconds,
                    // consider the headset disconnected/off.
                    if currently_connected && last_battery_time.elapsed() > Duration::from_secs(30) {
                        if debug {
                            eprintln!("[DEBUG] No battery packet received for 30s. Marking headset disconnected.");
                        }
                        currently_connected = false;
                        let _ = event_tx.blocking_send(HidEvent::HeadsetDisconnected);
                    }
                }
                Ok(n) => {
                    let packet = &buf[..n];
                    if debug {
                        eprintln!("[DEBUG] Raw incoming packet (len={n}): {:02x?}", packet);
                    }

                    match parse_packet(packet) {
                        PacketEvent::Battery(status) => {
                            last_battery_time = Instant::now();
                            currently_connected = status.connected;

                            if debug {
                                eprintln!(
                                    "[DEBUG] Decoded status: connected={}, charging={}, battery={}%",
                                    status.connected, status.charging, status.battery
                                );
                            }
                            let _ = event_tx.blocking_send(HidEvent::Status(status));
                        }
                        PacketEvent::PowerOrMute => {
                            if debug {
                                eprintln!("[DEBUG] Power/Mute event (len 2): {:02x?}", packet);
                            }
                        }
                        PacketEvent::Volume => {
                            if debug {
                                eprintln!("[DEBUG] Volume event (len 5): {:02x?}", packet);
                            }
                        }
                        PacketEvent::Ignored(len) => {
                            if debug {
                                eprintln!("[DEBUG] Ignored packet (len {len}): {:02x?}", packet);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[hyperx-battery] HID read error: {e}");
                    device = None;
                    currently_connected = false;
                    let _ = event_tx.blocking_send(HidEvent::DongleNotFound);
                    thread::sleep(Duration::from_millis(1500));
                }
            }
        }
    });
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let debug = args.iter().any(|arg| arg == "--debug" || arg == "-d");

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return;
    }

    if debug {
        eprintln!("[DEBUG] Debug logging enabled. Starting event-driven HyperX Cloud Flight battery monitor...");
    }

    // Channel for manual tray refresh trigger
    let (refresh_tx, refresh_rx) = std_mpsc::channel();

    // Spawn tray
    let tray = HeadsetTray::new(refresh_tx);
    let tray_handle = match tray.assume_sni_available(true).spawn().await {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("[hyperx-battery] Failed to spawn system tray: {e}");
            eprintln!("[hyperx-battery] Ensure your desktop environment supports StatusNotifierItem (e.g. GNOME AppIndicator).");
            return;
        }
    };

    // Channel for incoming HID events from background thread
    let (event_tx, mut event_rx) = mpsc::channel(32);

    // Spawn worker thread listening for unsolicited packets
    spawn_hid_worker(event_tx, refresh_rx, debug);

    let mut tracker = NotificationTracker::new();

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                let new_status = match event {
                    HidEvent::Status(status) => {
                        if status.connected {
                            tracker.update(&status);
                            TrayStatus::Connected {
                                battery: status.battery,
                                charging: status.charging,
                            }
                        } else {
                            TrayStatus::Disconnected
                        }
                    }
                    HidEvent::DongleNotFound => {
                        TrayStatus::DongleNotFound
                    }
                    HidEvent::HeadsetDisconnected => {
                        TrayStatus::Disconnected
                    }
                };

                tray_handle
                    .update(|tray| {
                        tray.status = new_status;
                    })
                    .await;
            }
            _ = tokio::signal::ctrl_c() => {
                if debug {
                    eprintln!("[DEBUG] Received SIGINT/Ctrl+C. Exiting cleanly.");
                }
                break;
            }
        }
    }
}
