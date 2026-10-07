# HyperX Cloud Flight Battery Monitor (Linux Tray App)

A lightweight Linux system tray application written in Rust that reads battery and charging status from a **HyperX Cloud Flight Wireless** headset via its USB dongle. No proprietary software or NGENUITY required.

---

## Features

- **System Tray Display**: Displays live headset connection and battery percentage in your status bar.
- **Charging Indicator**: Displays lightning bolt icon (`⚡`) when charging.
- **Desktop Notifications**:
  - Warns when battery drops to $\le 20\%$ (`HyperX: Low battery (X%)`).
  - Alerts when battery reaches $100\%$ while charging (`HyperX: Fully charged`).
  - One-shot alerts that automatically reset when levels recover (prevents spam).
- **Right-Click Context Menu**:
  - **Refresh**: Instantly triggers an immediate headset poll.
  - **Quit**: Exits the application cleanly.
- **Crash-Proof Design**:
  - Dongle unplugged? Shows `🎧 ✗` and continues polling every 30s.
  - Headset off / out of range? Shows `🎧 --` and continues polling.
  - Errors logged quietly to `stderr`, never spamming the user with popups.
- **Hardware Revision Support**:
  - Automatically queries primary Product ID `0x1723` and fallback revisions `0x1724` and `0x16c4`.

---

## Tray Display Reference

| State | Tray Label | Description |
|---|---|---|
| Connected (Normal) | `🎧 73%` | Headset connected, discharging |
| Connected (Charging) | `🎧 73% ⚡` | Headset connected and charging via cable |
| Disconnected | `🎧 --` | Dongle plugged in, headset powered off or out of range |
| Dongle Missing | `🎧 ✗` | USB dongle not plugged in or unreadable |

---

## Desktop Environment Requirements

This application implements the freedesktop **StatusNotifierItem** (SNI) specification via `ksni`.

- **KDE Plasma / XFCE / LXQt / Waybar / Polybar**: Supported natively out of the box.
- **GNOME**: By default, modern GNOME does not show system tray icons. **GNOME users must install the [AppIndicator and KStatusNotifierItem Support](https://extensions.gnome.org/extension/615/appindicator-support/) extension** for tray icons to appear in the top bar.

---

## udev Rule Setup (Required for Non-Root Access)

USB HID raw devices (`/dev/hidraw*`) are restricted to `root` by default on Linux. Install the included udev rule to allow normal users to communicate with the dongle.

1. Copy the rule to `/etc/udev/rules.d/`:
   ```bash
   sudo cp 99-hyperx.rules /etc/udev/rules.d/99-hyperx.rules
   ```

2. Reload and trigger udev:
   ```bash
   sudo udevadm control --reload-rules && sudo udevadm trigger
   ```

3. **Re-plug the USB dongle** for the new permissions to take effect.

> [!TIP]
> You can verify non-root access by inspecting `/dev/hidraw*` permissions:
> ```bash
> ls -l /dev/hidraw*
> ```
> The device corresponding to your HyperX dongle should now display permissions `crw-rw-rw-`.

---

## Prerequisites & Building

### Prerequisites

- **Rust toolchain** (1.75+ recommended):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Build libraries** (if using standard C-based hidapi):
  ```bash
  sudo apt install -y libudev-dev pkg-config
  ```

### Build from Source

```bash
cargo build --release
```

The compiled binary will be located at:
```bash
target/release/hyperx-battery
```

---

## Running

Run the binary directly:
```bash
./target/release/hyperx-battery
```

### Debug Mode

To inspect raw outgoing/incoming HID packets and diagnose communication issues on physical hardware, run with `--debug` or `-d`:
```bash
./target/release/hyperx-battery --debug
```

Example debug output:
```text
[DEBUG] Debug logging enabled. Starting HyperX Cloud Flight battery monitor...
[DEBUG] Connected to HyperX dongle (VID: 0x0951, PID: 0x1723)
[DEBUG] Sending HID request (20 bytes): [21, ff, 05, 00, 00, ...]
[DEBUG] get_feature_report returned 20 bytes
[DEBUG] Raw response buffer: [21, 00, 00, 01, 00, 49, ...]
[DEBUG] Parsed status: connected=true, charging=false, battery=73%
```

---

## Running Unit Tests

Run the built-in unit tests to verify packet parsing and notification state thresholds:
```bash
cargo test
```

---

## License

MIT License.
