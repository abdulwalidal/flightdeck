use hidapi::{HidApi, HidDevice};
use thiserror::Error;

/// Vendor ID for Kingston / HyperX
pub const HYPERX_VENDOR_ID: u16 = 0x0951;

/// Supported Product IDs for Cloud Flight dongle revisions
pub const SUPPORTED_PRODUCT_IDS: &[u16] = &[0x1723, 0x1724, 0x16c4];

/// Target HID usage page and usage for the 20-byte status interface
pub const STATUS_USAGE_PAGE: u16 = 0xff43;
pub const STATUS_USAGE: u16 = 0x0303;

/// Size of the bootstrap request packet
pub const BOOTSTRAP_PACKET_SIZE: usize = 20;

/// Headset connection and battery status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadsetStatus {
    pub connected: bool,
    pub charging: bool,
    pub battery: u8, // 0–100%
}

/// Events parsed from incoming unsolicited HID packets
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketEvent {
    Battery(HeadsetStatus),
    PowerOrMute,
    Volume,
    Ignored(usize),
}

/// High-level event sent to the main tray loop
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidEvent {
    Status(HeadsetStatus),
    DongleNotFound,
    HeadsetDisconnected,
}

/// Errors occurring during HID device operations
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HidError {
    #[error("HyperX Cloud Flight dongle not found")]
    DeviceNotFound,

    #[error("HID communication error: {0}")]
    ReadError(String),
}

/// Builds the 20-byte bootstrap trigger packet:
/// [0x21, 0xFF, 0x05, 0x00, ...zeros]
/// Sent once upon connection to wake up the dongle's status stream.
pub fn build_bootstrap_packet() -> [u8; BOOTSTRAP_PACKET_SIZE] {
    let mut packet = [0u8; BOOTSTRAP_PACKET_SIZE];
    packet[0] = 0x21;
    packet[1] = 0xff;
    packet[2] = 0x05;
    packet
}

/// Calculates charging status and battery percentage using the reverse-engineered lookup table.
///
/// Charge state tier (tier) and raw value:
/// - charging = tier == 0x10 && value >= 20
/// - 0x10: 100%
/// - 0x0f:
///   - >= 130: 100%
///   - >= 120: 95%
///   - >= 100: 90%
///   - >= 70:  85%
///   - >= 50:  80%
///   - >= 20:  75%
///   - > 0:    70%
/// - 0x0e:
///   - >= 240: 65%
///   - >= 220: 60%
///   - >= 208: 55%
///   - >= 200: 50%
///   - >= 190: 45%
///   - >= 180: 40%
///   - >= 169: 35%
///   - >= 159: 30%
///   - >= 148: 25%
///   - >= 119: 20%
///   - >= 90:  15%
///   - < 90:   10%
pub fn calculate_battery(tier: u8, value: u8) -> (bool, u8) {
    let charging = tier == 0x10 && value >= 20;

    let battery = match tier {
        t if t >= 0x10 => 100,
        0x0f => {
            if value >= 130 {
                100
            } else if value >= 120 {
                95
            } else if value >= 100 {
                90
            } else if value >= 70 {
                85
            } else if value >= 50 {
                80
            } else if value >= 20 {
                75
            } else {
                70
            }
        }
        0x0e => {
            if value >= 240 {
                65
            } else if value >= 220 {
                60
            } else if value >= 208 {
                55
            } else if value >= 200 {
                50
            } else if value >= 190 {
                45
            } else if value >= 180 {
                40
            } else if value >= 169 {
                35
            } else if value >= 159 {
                30
            } else if value >= 148 {
                25
            } else if value >= 119 {
                20
            } else if value >= 90 {
                15
            } else {
                10
            }
        }
        0x0d => 5,
        _ => 0,
    };

    (charging, battery)
}

/// Parses an unsolicited HID packet from the Cloud Flight dongle.
///
/// Packet formats:
/// - Length 2: Power/mute events
/// - Length 5: Volume events
/// - Length 20 (0x14) or 15 (0x0f): Battery status packet:
///   - data[3] = charge tier
///   - data[4] = raw value
pub fn parse_packet(data: &[u8]) -> PacketEvent {
    match data.len() {
        2 => PacketEvent::PowerOrMute,
        5 => PacketEvent::Volume,
        15 | 20 if data.len() >= 5 => {
            let tier = data[3];
            let value = data[4];

            if tier == 0 {
                // Tier 0 indicates disconnected headset
                PacketEvent::Battery(HeadsetStatus {
                    connected: false,
                    charging: false,
                    battery: 0,
                })
            } else {
                let (charging, battery) = calculate_battery(tier, value);
                PacketEvent::Battery(HeadsetStatus {
                    connected: true,
                    charging,
                    battery,
                })
            }
        }
        n => PacketEvent::Ignored(n),
    }
}

/// Finds and opens the Cloud Flight status HID interface.
///
/// Prefers interface matching usagePage 0xFF43 and usage 0x0303.
/// If usage properties are unavailable from the driver, falls back to opening
/// candidate devices matching the vendor/product IDs.
pub fn open_status_interface(debug: bool) -> Result<HidDevice, HidError> {
    let api = HidApi::new().map_err(|e| HidError::ReadError(format!("Failed to init HidApi: {e}")))?;

    let mut matched_device_path = None;
    let mut fallback_path = None;

    for info in api.device_list() {
        if info.vendor_id() == HYPERX_VENDOR_ID
            && SUPPORTED_PRODUCT_IDS.contains(&info.product_id())
        {
            if debug {
                eprintln!(
                    "[DEBUG] Found device VID: 0x{:04x}, PID: 0x{:04x}, interface: {}, usage_page: 0x{:04x}, usage: 0x{:04x}",
                    info.vendor_id(),
                    info.product_id(),
                    info.interface_number(),
                    info.usage_page(),
                    info.usage()
                );
            }

            // Check for explicit status interface usage
            if info.usage_page() == STATUS_USAGE_PAGE && info.usage() == STATUS_USAGE {
                matched_device_path = Some(info.path().to_owned());
                break;
            }

            // Keep fallback path in case driver doesn't expose usage_page / usage
            if fallback_path.is_none() {
                fallback_path = Some(info.path().to_owned());
            }
        }
    }

    let target_path = matched_device_path.or(fallback_path);

    let path = match target_path {
        Some(p) => p,
        None => return Err(HidError::DeviceNotFound),
    };

    if debug {
        eprintln!("[DEBUG] Opening HID device path: {:?}", path);
    }

    api.open_path(&path)
        .map_err(|e| HidError::ReadError(format!("Failed to open device at path: {e}")))
}

/// Sends the initial bootstrap packet to the dongle to wake up the status stream.
pub fn send_bootstrap(device: &HidDevice, debug: bool) {
    let bootstrap = build_bootstrap_packet();
    if debug {
        eprintln!("[DEBUG] Sending bootstrap trigger packet: {:02x?}", bootstrap);
    }

    // Try feature report first, then write fallback
    if let Err(e) = device.send_feature_report(&bootstrap) {
        if debug {
            eprintln!("[DEBUG] send_feature_report bootstrap failed ({e}), trying write()");
        }
        let _ = device.write(&bootstrap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_bootstrap_packet() {
        let packet = build_bootstrap_packet();
        assert_eq!(packet.len(), 20);
        assert_eq!(packet[0], 0x21);
        assert_eq!(packet[1], 0xff);
        assert_eq!(packet[2], 0x05);
        for &byte in &packet[3..] {
            assert_eq!(byte, 0x00);
        }
    }

    #[test]
    fn test_calculate_battery_tier_10_charging() {
        let (charging, battery) = calculate_battery(0x10, 20);
        assert!(charging);
        assert_eq!(battery, 100);

        let (charging, battery) = calculate_battery(0x10, 50);
        assert!(charging);
        assert_eq!(battery, 100);

        let (charging, battery) = calculate_battery(0x10, 15);
        assert!(!charging);
        assert_eq!(battery, 100);
    }

    #[test]
    fn test_calculate_battery_tier_0f() {
        assert_eq!(calculate_battery(0x0f, 135), (false, 100));
        assert_eq!(calculate_battery(0x0f, 125), (false, 95));
        assert_eq!(calculate_battery(0x0f, 105), (false, 90));
        assert_eq!(calculate_battery(0x0f, 75), (false, 85));
        assert_eq!(calculate_battery(0x0f, 55), (false, 80));
        assert_eq!(calculate_battery(0x0f, 25), (false, 75));
        assert_eq!(calculate_battery(0x0f, 5), (false, 70));
    }

    #[test]
    fn test_calculate_battery_tier_0e() {
        assert_eq!(calculate_battery(0x0e, 245), (false, 65));
        assert_eq!(calculate_battery(0x0e, 225), (false, 60));
        assert_eq!(calculate_battery(0x0e, 210), (false, 55));
        assert_eq!(calculate_battery(0x0e, 205), (false, 50));
        assert_eq!(calculate_battery(0x0e, 195), (false, 45));
        assert_eq!(calculate_battery(0x0e, 185), (false, 40));
        assert_eq!(calculate_battery(0x0e, 170), (false, 35));
        assert_eq!(calculate_battery(0x0e, 160), (false, 30));
        assert_eq!(calculate_battery(0x0e, 150), (false, 25));
        assert_eq!(calculate_battery(0x0e, 120), (false, 20));
        assert_eq!(calculate_battery(0x0e, 100), (false, 15));
        assert_eq!(calculate_battery(0x0e, 50), (false, 10));
    }

    #[test]
    fn test_parse_packet_lengths() {
        // Power/mute event
        assert_eq!(parse_packet(&[0x65, 0x01]), PacketEvent::PowerOrMute);

        // Volume event
        assert_eq!(parse_packet(&[0x01, 0x02, 0x03, 0x04, 0x05]), PacketEvent::Volume);

        // 20-byte battery packet (tier 0x0f, value 125 -> 95%)
        let mut data20 = [0u8; 20];
        data20[3] = 0x0f;
        data20[4] = 125;
        assert_eq!(
            parse_packet(&data20),
            PacketEvent::Battery(HeadsetStatus {
                connected: true,
                charging: false,
                battery: 95,
            })
        );

        // 15-byte battery packet (tier 0x10, value 30 -> charging, 100%)
        let mut data15 = [0u8; 15];
        data15[3] = 0x10;
        data15[4] = 30;
        assert_eq!(
            parse_packet(&data15),
            PacketEvent::Battery(HeadsetStatus {
                connected: true,
                charging: true,
                battery: 100,
            })
        );

        // Disconnected (tier 0)
        let mut disconnected = [0u8; 20];
        disconnected[3] = 0x00;
        assert_eq!(
            parse_packet(&disconnected),
            PacketEvent::Battery(HeadsetStatus {
                connected: false,
                charging: false,
                battery: 0,
            })
        );
    }
}
