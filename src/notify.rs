use crate::hid::HeadsetStatus;
use notify_rust::Notification;

/// Manages desktop notification state to prevent repeated alerts on every poll tick.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NotificationTracker {
    pub low_battery_notified: bool,
    pub fully_charged_notified: bool,
}

impl NotificationTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates the headset status and dispatches desktop notifications if a threshold was crossed.
    pub fn update(&mut self, status: &HeadsetStatus) {
        if !status.connected {
            // Do not send battery notifications if headset is disconnected
            return;
        }

        // Low battery notification (<= 20%)
        if status.battery <= 20 {
            if !self.low_battery_notified {
                self.send_low_battery_notification(status.battery);
                self.low_battery_notified = true;
            }
        } else {
            // Reset when battery recovers above 20%
            self.low_battery_notified = false;
        }

        // Fully charged notification (100% while charging)
        if status.battery == 100 && status.charging {
            if !self.fully_charged_notified {
                self.send_fully_charged_notification();
                self.fully_charged_notified = true;
            }
        } else {
            // Reset if battery is no longer 100% or not charging
            self.fully_charged_notified = false;
        }
    }

    fn send_low_battery_notification(&self, battery: u8) {
        let body = format!("HyperX: Low battery ({}%)", battery);
        Self::dispatch("HyperX Cloud Flight", &body);
    }

    fn send_fully_charged_notification(&self) {
        Self::dispatch("HyperX Cloud Flight", "HyperX: Fully charged");
    }

    fn dispatch(summary: &str, body: &str) {
        if let Err(e) = Notification::new()
            .appname("HyperX Battery")
            .summary(summary)
            .body(body)
            .icon("audio-headset")
            .show()
        {
            eprintln!("[hyperx-battery] Failed to send desktop notification: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_low_battery_threshold_and_reset() {
        let mut tracker = NotificationTracker::new();

        // Battery at 80%: no notification
        let normal = HeadsetStatus {
            connected: true,
            charging: false,
            battery: 80,
        };
        tracker.update(&normal);
        assert!(!tracker.low_battery_notified);

        // Drops to 20%: triggers low battery notification
        let low = HeadsetStatus {
            connected: true,
            charging: false,
            battery: 20,
        };
        tracker.update(&low);
        assert!(tracker.low_battery_notified);

        // Second poll at 19%: still low, should not trigger new notification
        let still_low = HeadsetStatus {
            connected: true,
            charging: false,
            battery: 19,
        };
        tracker.update(&still_low);
        assert!(tracker.low_battery_notified);

        // Charged back to 25%: resets low battery flag
        let recovered = HeadsetStatus {
            connected: true,
            charging: true,
            battery: 25,
        };
        tracker.update(&recovered);
        assert!(!tracker.low_battery_notified);

        // Drops again to 15%: triggers again
        let low_again = HeadsetStatus {
            connected: true,
            charging: false,
            battery: 15,
        };
        tracker.update(&low_again);
        assert!(tracker.low_battery_notified);
    }

    #[test]
    fn test_fully_charged_threshold_and_reset() {
        let mut tracker = NotificationTracker::new();

        // 99% charging: no fully charged notification
        let almost_full = HeadsetStatus {
            connected: true,
            charging: true,
            battery: 99,
        };
        tracker.update(&almost_full);
        assert!(!tracker.fully_charged_notified);

        // 100% charging: triggers notification
        let full_charging = HeadsetStatus {
            connected: true,
            charging: true,
            battery: 100,
        };
        tracker.update(&full_charging);
        assert!(tracker.fully_charged_notified);

        // Second poll still 100% charging: stays notified, no repeat
        tracker.update(&full_charging);
        assert!(tracker.fully_charged_notified);

        // Unplugged charger (100% not charging): resets fully charged flag
        let full_unplugged = HeadsetStatus {
            connected: true,
            charging: false,
            battery: 100,
        };
        tracker.update(&full_unplugged);
        assert!(!tracker.fully_charged_notified);
    }

    #[test]
    fn test_disconnected_does_not_notify() {
        let mut tracker = NotificationTracker::new();

        let disconnected = HeadsetStatus {
            connected: false,
            charging: false,
            battery: 0,
        };
        tracker.update(&disconnected);
        assert!(!tracker.low_battery_notified);
        assert!(!tracker.fully_charged_notified);
    }
}
