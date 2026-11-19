use std::fmt;

use crate::error::DomainError;

/// Stable identifier reported by ADB for one device.
///
/// Invariant: non-empty and free of whitespace and control characters. Use
/// [`Serial::new`] to build one; the inner value is private so the invariant
/// cannot be bypassed accidentally.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Serial(String);

impl Serial {
    /// Builds a validated serial.
    pub fn new(raw: impl Into<String>) -> Result<Self, DomainError> {
        let value = raw.into();
        if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(DomainError::InvalidSerial);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Serial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for Serial {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// ADB authorization state of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceState {
    Connected,
    Unauthorized,
    Offline,
}

/// Native screen size reported by the device.
///
/// Invariant: both dimensions are greater than zero. Prefer [`Resolution::new`]
/// over a struct literal; the fields stay public to match the M0 contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    pub fn new(width: u32, height: u32) -> Result<Self, DomainError> {
        if width == 0 || height == 0 {
            return Err(DomainError::InvalidResolution);
        }
        Ok(Self { width, height })
    }
}

/// A single Android device as exposed to the rest of the application.
///
/// Invariants: `name` is not blank and `battery_percent`, when present, is
/// between 0 and 100. Use [`Device::new`] to build one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: Serial,
    pub name: String,
    pub state: DeviceState,
    pub android_version: Option<String>,
    pub resolution: Option<Resolution>,
    pub battery_percent: Option<u8>,
}

impl Device {
    pub fn new(
        serial: Serial,
        name: impl Into<String>,
        state: DeviceState,
        android_version: Option<String>,
        resolution: Option<Resolution>,
        battery_percent: Option<u8>,
    ) -> Result<Self, DomainError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(DomainError::EmptyName);
        }
        if let Some(percent) = battery_percent {
            if percent > 100 {
                return Err(DomainError::InvalidBattery);
            }
        }
        Ok(Self {
            serial,
            name,
            state,
            android_version,
            resolution,
            battery_percent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serial(raw: &str) -> Serial {
        Serial::new(raw).expect("test serial should be valid")
    }

    #[test]
    fn serial_accepts_typical_adb_serials() {
        for raw in ["R58M12345", "1234567890ABC", "0123456789ABCDEF"] {
            let value = Serial::new(raw).expect("serial should be valid");
            assert_eq!(value.as_str(), raw);
            assert_eq!(value.to_string(), raw);
        }
    }

    #[test]
    fn serial_rejects_empty_value() {
        assert_eq!(Serial::new(""), Err(DomainError::InvalidSerial));
    }

    #[test]
    fn serial_rejects_whitespace() {
        for raw in ["R58 M12345", " R58M12345", "R58M12345\n", "R58M\t12345"] {
            assert_eq!(
                Serial::new(raw),
                Err(DomainError::InvalidSerial),
                "{raw:?} should be rejected"
            );
        }
    }

    #[test]
    fn serial_rejects_control_characters() {
        assert_eq!(
            Serial::new("R58\u{0}M12345"),
            Err(DomainError::InvalidSerial)
        );
    }

    #[test]
    fn resolution_rejects_zero_dimensions() {
        assert_eq!(
            Resolution::new(0, 2400),
            Err(DomainError::InvalidResolution)
        );
        assert_eq!(
            Resolution::new(1080, 0),
            Err(DomainError::InvalidResolution)
        );
        assert_eq!(Resolution::new(0, 0), Err(DomainError::InvalidResolution));
    }

    #[test]
    fn resolution_accepts_positive_dimensions() {
        let resolution = Resolution::new(1080, 2400).expect("resolution should be valid");
        assert_eq!(resolution.width, 1080);
        assert_eq!(resolution.height, 2400);
    }

    #[test]
    fn device_rejects_blank_name() {
        for name in ["", "   ", "\t\n"] {
            assert_eq!(
                Device::new(
                    serial("R58M12345"),
                    name,
                    DeviceState::Connected,
                    None,
                    None,
                    None
                ),
                Err(DomainError::EmptyName),
                "{name:?} should be rejected"
            );
        }
    }

    #[test]
    fn device_rejects_out_of_range_battery() {
        assert_eq!(
            Device::new(
                serial("R58M12345"),
                "Pixel 8 Pro",
                DeviceState::Connected,
                None,
                None,
                Some(101),
            ),
            Err(DomainError::InvalidBattery)
        );
        assert_eq!(
            Device::new(
                serial("R58M12345"),
                "Pixel 8 Pro",
                DeviceState::Connected,
                None,
                None,
                Some(255),
            ),
            Err(DomainError::InvalidBattery)
        );
    }

    #[test]
    fn device_accepts_optional_details() {
        let device = Device::new(
            serial("R58M12345"),
            "Pixel 8 Pro",
            DeviceState::Connected,
            Some("15".to_owned()),
            Some(Resolution::new(1080, 2400).expect("valid resolution")),
            Some(85),
        )
        .expect("device should be valid");

        assert_eq!(device.serial.as_str(), "R58M12345");
        assert_eq!(device.name, "Pixel 8 Pro");
        assert_eq!(device.state, DeviceState::Connected);
        assert_eq!(device.android_version.as_deref(), Some("15"));
        assert_eq!(device.resolution.expect("resolution").width, 1080);
        assert_eq!(device.battery_percent, Some(85));
    }

    #[test]
    fn device_accepts_missing_optional_details() {
        let device = Device::new(
            serial("1234567890ABC"),
            "  Pixel 8 Pro  ",
            DeviceState::Unauthorized,
            None,
            None,
            None,
        )
        .expect("device should be valid");

        assert_eq!(device.name, "  Pixel 8 Pro  ");
        assert_eq!(device.state, DeviceState::Unauthorized);
        assert!(device.android_version.is_none());
        assert!(device.resolution.is_none());
        assert!(device.battery_percent.is_none());
    }

    #[test]
    fn device_accepts_battery_bounds() {
        for percent in [0, 100] {
            let device = Device::new(
                serial("R58M12345"),
                "Pixel 8 Pro",
                DeviceState::Connected,
                None,
                None,
                Some(percent),
            )
            .expect("boundary battery value should be valid");
            assert_eq!(device.battery_percent, Some(percent));
        }
    }
}
