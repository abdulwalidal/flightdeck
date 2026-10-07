use hidapi::HidApi;
use thiserror::Error;

/// Vendor ID for Kingston / HyperX
pub const HYPERX_VENDOR_ID: u16 = 0x0951;

/// Product IDs for HyperX Cloud Flight Wireless dongles:
/// - 0x1723: Standard Cloud Flight Wireless dongle
/// - 0x1724: Alternate revision Cloud Flight Wireless dongle
/// - 0x16c4: Cloud Flight Wireless revision
pub const SUPPORTED_PRODUCT_IDS: &[u16] = &[0x1723, 0x1724, 0x16c4];

/// Size of the HID report packets for Cloud Flight
pub const REPORT_SIZE: usize = 20;

/// Headset connection and battery status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadsetStatus {
    pub connected: bool,
    pub charging: bool,
    pub battery: u8, // 0–100%
}

/// Errors occurring during HID device communication
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HidError {
    #[error("HyperX Cloud Flight dongle not found")]
    DeviceNotFound,

    #[error("HID communication error: {0}")]
    ReadError(String),
}

/// Builds the 20-byte feature request packet:
/// [0x21, 0xFF, 0x05, 0x00, ...padding zeros]
pub fn build_request_packet() -> [u8; REPORT_SIZE] {
    let mut packet = [0u8; REPORT_SIZE];
    packet[0] = 0x21;
    packet[1] = 0xff;
    packet[2] = 0x05;
    packet
}

/// Parses the 20-byte HID response packet from the headset dongle:
/// - byte[0] = report ID (ignored)
/// - byte[3] = connection status -> 0x01 = connected, 0x00 = disconnected
/// - byte[4] = charging status   -> 0x01 = charging, 0x00 = not charging
/// - byte[5] = battery level     -> 0–100 (direct percentage)
pub fn parse_response(data: &[u8]) -> Result<HeadsetStatus, HidError> {
    if data.len() < 6 {
        return Err(HidError::ReadError(format!(
            "Response buffer too small (expected at least 6 bytes, got {})",
            data.len()
        )));
    }

    let connected = data[3] == 0x01;
    let charging = data[4] == 0x01;
    let battery = data[5].min(100);

    Ok(HeadsetStatus {
        connected,
        charging,
        battery,
    })
}

/// Polls the headset state via HID USB dongle.
///
/// Tries primary PID (0x1723) and fallbacks (0x1724, 0x16c4).
/// If `debug` is enabled, raw outgoing and incoming bytes are printed to stderr.
pub fn poll_headset(debug: bool) -> Result<HeadsetStatus, HidError> {
    let api = HidApi::new().map_err(|e| HidError::ReadError(format!("Failed to init HidApi: {e}")))?;

    // Search for any supported device
    let mut device_opt = None;
    let mut matched_pid = 0;

    for &pid in SUPPORTED_PRODUCT_IDS {
        if let Ok(dev) = api.open(HYPERX_VENDOR_ID, pid) {
            device_opt = Some(dev);
            matched_pid = pid;
            break;
        }
    }

    let device = match device_opt {
        Some(dev) => dev,
        None => return Err(HidError::DeviceNotFound),
    };

    if debug {
        eprintln!(
            "[DEBUG] Connected to HyperX dongle (VID: 0x{:04x}, PID: 0x{:04x})",
            HYPERX_VENDOR_ID, matched_pid
        );
    }

    let request = build_request_packet();

    if debug {
        eprintln!("[DEBUG] Sending HID request ({} bytes): {:02x?}", request.len(), request);
    }

    // Attempt feature report write first per spec
    let write_res = device.send_feature_report(&request);
    if let Err(ref e) = write_res {
        if debug {
            eprintln!("[DEBUG] send_feature_report failed ({e}), falling back to device.write()");
        }
        // Fallback to write() if send_feature_report is not supported by driver
        device
            .write(&request)
            .map_err(|err| HidError::ReadError(format!("Failed to write request: {err}")))?;
    }

    let mut response = [0u8; REPORT_SIZE];
    response[0] = 0x21; // Set report ID for feature report query

    // Try reading feature report
    let mut read_bytes = 0;
    if let Ok(n) = device.get_feature_report(&mut response) {
        read_bytes = n;
        if debug {
            eprintln!("[DEBUG] get_feature_report returned {n} bytes");
        }
    }

    // If get_feature_report was empty or failed, try read_timeout
    if read_bytes < 6 {
        match device.read_timeout(&mut response, 500) {
            Ok(n) => {
                read_bytes = n;
                if debug {
                    eprintln!("[DEBUG] read_timeout returned {n} bytes");
                }
            }
            Err(e) => {
                if read_bytes == 0 {
                    return Err(HidError::ReadError(format!("Failed to read response: {e}")));
                }
            }
        }
    }

    if debug {
        eprintln!(
            "[DEBUG] Raw response buffer ({} bytes read): {:02x?}",
            read_bytes,
            &response[..read_bytes.max(6).min(REPORT_SIZE)]
        );
    }

    if read_bytes < 6 {
        return Err(HidError::ReadError(format!(
            "Insufficient bytes received from dongle (got {read_bytes}, need >= 6)"
        )));
    }

    let status = parse_response(&response)?;

    if debug {
        eprintln!(
            "[DEBUG] Parsed status: connected={}, charging={}, battery={}%",
            status.connected, status.charging, status.battery
        );
    }

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_request_packet() {
        let packet = build_request_packet();
        assert_eq!(packet.len(), 20);
        assert_eq!(packet[0], 0x21);
        assert_eq!(packet[1], 0xff);
        assert_eq!(packet[2], 0x05);
        for &byte in &packet[3..] {
            assert_eq!(byte, 0x00);
        }
    }

    #[test]
    fn test_parse_response_connected_not_charging() {
        let mut data = [0u8; 20];
        data[0] = 0x21;
        data[3] = 0x01; // Connected
        data[4] = 0x00; // Not charging
        data[5] = 73;   // 73%

        let status = parse_response(&data).expect("Should parse successfully");
        assert_eq!(
            status,
            HeadsetStatus {
                connected: true,
                charging: false,
                battery: 73,
            }
        );
    }

    #[test]
    fn test_parse_response_connected_charging() {
        let mut data = [0u8; 20];
        data[0] = 0x21;
        data[3] = 0x01; // Connected
        data[4] = 0x01; // Charging
        data[5] = 100;  // 100%

        let status = parse_response(&data).expect("Should parse successfully");
        assert_eq!(
            status,
            HeadsetStatus {
                connected: true,
                charging: true,
                battery: 100,
            }
        );
    }

    #[test]
    fn test_parse_response_disconnected() {
        let mut data = [0u8; 20];
        data[0] = 0x21;
        data[3] = 0x00; // Disconnected
        data[4] = 0x00;
        data[5] = 0;

        let status = parse_response(&data).expect("Should parse successfully");
        assert_eq!(
            status,
            HeadsetStatus {
                connected: false,
                charging: false,
                battery: 0,
            }
        );
    }

    #[test]
    fn test_parse_response_battery_clamp() {
        let mut data = [0u8; 20];
        data[0] = 0x21;
        data[3] = 0x01;
        data[4] = 0x01;
        data[5] = 150; // Malformed value above 100%

        let status = parse_response(&data).expect("Should clamp to 100");
        assert_eq!(status.battery, 100);
    }

    #[test]
    fn test_parse_response_buffer_too_short() {
        let data = [0x21, 0xff, 0x05];
        let err = parse_response(&data).expect_err("Should error on short buffer");
        match err {
            HidError::ReadError(msg) => assert!(msg.contains("Response buffer too small")),
            _ => panic!("Expected ReadError"),
        }
    }
}
