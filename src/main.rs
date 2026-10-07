mod hid;
mod notify;
mod tray;

use std::env;
use std::time::Duration;
use ksni::TrayMethods;
use notify::NotificationTracker;
use tokio::sync::mpsc;
use tray::{HeadsetTray, TrayStatus};

const POLL_INTERVAL_SECS: u64 = 30;

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

async fn perform_poll(
    debug: bool,
    tracker: &mut NotificationTracker,
    tray_handle: &ksni::Handle<HeadsetTray>,
) {
    let new_status = match hid::poll_headset(debug) {
        Ok(status) => {
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
        Err(hid::HidError::DeviceNotFound) => {
            eprintln!("[hyperx-battery] USB dongle not found");
            TrayStatus::DongleNotFound
        }
        Err(hid::HidError::ReadError(err)) => {
            eprintln!("[hyperx-battery] Read error: {err}");
            TrayStatus::Disconnected
        }
    };

    tray_handle
        .update(|tray| {
            tray.status = new_status;
        })
        .await;
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
        eprintln!("[DEBUG] Debug logging enabled. Starting HyperX Cloud Flight battery monitor...");
    }

    let (refresh_tx, mut refresh_rx) = mpsc::channel(1);
    let tray = HeadsetTray::new(refresh_tx);

    let tray_handle = match tray.assume_sni_available(true).spawn().await {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("[hyperx-battery] Failed to spawn system tray: {e}");
            eprintln!("[hyperx-battery] Ensure your desktop environment supports StatusNotifierItem (e.g. GNOME AppIndicator).");
            return;
        }
    };

    let mut tracker = NotificationTracker::new();

    // Initial poll immediately upon startup
    perform_poll(debug, &mut tracker, &tray_handle).await;

    let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
    // The first tick completes immediately; since we already did an initial poll, consume it
    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                perform_poll(debug, &mut tracker, &tray_handle).await;
            }
            Some(_) = refresh_rx.recv() => {
                if debug {
                    eprintln!("[DEBUG] Manual refresh triggered from tray menu");
                }
                perform_poll(debug, &mut tracker, &tray_handle).await;
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
