use ksni::menu::StandardItem;
use ksni::{MenuItem, ToolTip, Tray};
use std::sync::mpsc::Sender;

/// Current state represented in the tray
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayStatus {
    DongleNotFound,
    Disconnected,
    Connected { battery: u8, charging: bool },
}

/// System tray item conforming to StatusNotifierItem
pub struct HeadsetTray {
    pub status: TrayStatus,
    refresh_tx: Sender<()>,
}

impl HeadsetTray {
    pub fn new(refresh_tx: Sender<()>) -> Self {
        Self {
            status: TrayStatus::DongleNotFound,
            refresh_tx,
        }
    }
}

impl Tray for HeadsetTray {
    fn id(&self) -> String {
        "hyperx-battery".to_string()
    }

    fn title(&self) -> String {
        match &self.status {
            TrayStatus::Connected { battery, charging } => {
                if *charging {
                    format!("🎧 {battery}% ⚡")
                } else {
                    format!("🎧 {battery}%")
                }
            }
            TrayStatus::Disconnected => "🎧 --".to_string(),
            TrayStatus::DongleNotFound => "🎧 ✗".to_string(),
        }
    }

    fn icon_name(&self) -> String {
        // Standard freedesktop icon name for audio headsets
        "audio-headset".to_string()
    }

    fn tool_tip(&self) -> ToolTip {
        let (title, description) = match &self.status {
            TrayStatus::Connected { battery, charging } => (
                "HyperX Cloud Flight",
                format!("Battery: {battery}%{}", if *charging { " (Charging)" } else { "" }),
            ),
            TrayStatus::Disconnected => (
                "HyperX Cloud Flight",
                "Headset disconnected or turned off".to_string(),
            ),
            TrayStatus::DongleNotFound => (
                "HyperX Cloud Flight",
                "USB dongle not found".to_string(),
            ),
        };

        ToolTip {
            title: title.to_string(),
            description,
            icon_name: "audio-headset".to_string(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let refresh_tx = self.refresh_tx.clone();

        vec![
            StandardItem {
                label: "Refresh".to_string(),
                activate: Box::new(move |_| {
                    let _ = refresh_tx.send(());
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|_| {
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn test_tray_labels_per_spec() {
        let (tx, _rx) = mpsc::channel();
        let mut tray = HeadsetTray::new(tx);

        tray.status = TrayStatus::Connected {
            battery: 73,
            charging: false,
        };
        assert_eq!(tray.title(), "🎧 73%");

        tray.status = TrayStatus::Connected {
            battery: 73,
            charging: true,
        };
        assert_eq!(tray.title(), "🎧 73% ⚡");

        tray.status = TrayStatus::Disconnected;
        assert_eq!(tray.title(), "🎧 --");

        tray.status = TrayStatus::DongleNotFound;
        assert_eq!(tray.title(), "🎧 ✗");
    }
}
