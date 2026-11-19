use lotu_domain::{Device, DeviceState, Resolution, VideoConfig};
use serde::{Deserialize, Serialize, Serializer};

/// Wire representation of a device sent to the frontend.
///
/// `state` is serialized as a lowercase string by [`serialize_device_state`] so
/// the domain type does not need a `serde` dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub serial: String,
    pub name: String,
    #[serde(serialize_with = "serialize_device_state")]
    pub state: DeviceState,
    pub android_version: Option<String>,
    pub resolution: Option<ResolutionDto>,
    pub battery_percent: Option<u8>,
    /// The MVP only supports USB connections; this stays `"usb"`.
    pub transport: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResolutionDto {
    pub width: u32,
    pub height: u32,
}

/// Session handed to the frontend by `start_session` (live, M1) and by
/// `start_replay` (fixture, M0b).
///
/// `ws_url` already contains the loopback endpoint and the per-session token;
/// `device` is the `DeviceDto` of the catalog (the fixture metadata for replay).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub ws_url: String,
    pub device: DeviceDto,
}

/// The replay command keeps its own name in the frontend; the shape is
/// identical, so it is the same type.
pub type ReplaySessionDto = SessionDto;

/// M4: video settings as stored and as accepted by `save_settings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoSettingsDto {
    pub max_size: u32,
    pub max_fps: u32,
    pub bit_rate: u32,
}

impl From<VideoConfig> for VideoSettingsDto {
    fn from(config: VideoConfig) -> Self {
        Self {
            max_size: config.max_size,
            max_fps: config.max_fps,
            bit_rate: config.bit_rate,
        }
    }
}

/// M4: full settings payload. `restartRequired` is computed from the active session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub video: VideoSettingsDto,
    pub restart_required: bool,
}

/// Result of `capture_screenshot`. Width and height are the native ones read
/// from the PNG, not the dimensions of the streamed video (`docs/adr/0001`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotDto {
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// Result of starting or stopping a recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDto {
    pub path: String,
    pub frames: u64,
    pub duration_us: u64,
    /// More than one when the device rotated during the recording.
    pub segments: u32,
    /// Every produced MP4, in order.
    pub paths: Vec<String>,
}

/// Snapshot of the single recording slot, sent as a Tauri event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStateDto {
    pub active: bool,
    pub path: Option<String>,
}

/// Action of a pointer event (`pointerdown`, `pointermove`, `pointerup`).
///
/// Wire value in `docs/m2-contract.md` §3.1: lowercase strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PointerAction {
    Down,
    Move,
    Up,
}

/// Action of a key event. Used by keycodes and back-or-screen-on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyAction {
    Down,
    Up,
}

/// Touch injection in the coordinate space of the last video configuration.
///
/// `screen_width`/`screen_height` must be the dimensions of the last session
/// meta received over the stream WebSocket; otherwise the device silently drops
/// the event (`docs/m2-contract.md` §2.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerInput {
    pub action: PointerAction,
    pub x: i32,
    pub y: i32,
    pub screen_width: u16,
    pub screen_height: u16,
    /// `0.0..=1.0`; M2 only sends fully pressed or released.
    pub pressure: f32,
    /// Android button bitmask (`android/input.h`); M2 only uses `0` or `1`.
    pub buttons: u32,
    /// Button whose state changed with this event; M2 only uses `0` or `1`.
    pub action_button: u32,
}

/// Scroll (wheel) injection in the coordinate space of the last video config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollInput {
    pub x: i32,
    pub y: i32,
    pub screen_width: u16,
    pub screen_height: u16,
    /// Horizontal notches, `[-16, 16]`; positive scrolls right.
    pub h_scroll: f32,
    /// Vertical notches, `[-16, 16]`; positive scrolls up.
    pub v_scroll: f32,
    /// Android button bitmask; M2 sends `0`.
    pub buttons: u32,
}

/// Keycode injection (Android `KeyEvent` semantics).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInput {
    pub action: KeyAction,
    pub keycode: i32,
    /// Repeat count; M2 always sends `0` (`docs/m2-contract.md` §2.3).
    pub repeat: i32,
    /// Android `META_*` bitmask.
    pub meta_state: i32,
}

impl From<Device> for DeviceDto {
    fn from(device: Device) -> Self {
        Self {
            serial: device.serial.as_str().to_owned(),
            name: device.name,
            state: device.state,
            android_version: device.android_version,
            resolution: device.resolution.map(ResolutionDto::from),
            battery_percent: device.battery_percent,
            transport: "usb",
        }
    }
}

impl From<Resolution> for ResolutionDto {
    fn from(resolution: Resolution) -> Self {
        Self {
            width: resolution.width,
            height: resolution.height,
        }
    }
}

fn serialize_device_state<S>(state: &DeviceState, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(match state {
        DeviceState::Connected => "connected",
        DeviceState::Unauthorized => "unauthorized",
        DeviceState::Offline => "offline",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotu_domain::Serial;

    fn device(state: DeviceState) -> Device {
        Device::new(
            Serial::new("R58M12345").expect("valid serial"),
            "Pixel 8 Pro",
            state,
            Some("15".to_owned()),
            Some(Resolution::new(1080, 2400).expect("valid resolution")),
            Some(85),
        )
        .expect("valid device")
    }

    #[test]
    fn dto_serializes_camel_case_fields() {
        let dto = DeviceDto::from(device(DeviceState::Connected));
        let value = serde_json::to_value(&dto).expect("dto should serialize");

        assert_eq!(value["serial"], "R58M12345");
        assert_eq!(value["name"], "Pixel 8 Pro");
        assert_eq!(value["androidVersion"], "15");
        assert_eq!(value["batteryPercent"], 85);
        assert_eq!(value["transport"], "usb");
        assert_eq!(value["resolution"]["width"], 1080);
        assert_eq!(value["resolution"]["height"], 2400);
        assert!(value.get("android_version").is_none());
        assert!(value.get("battery_percent").is_none());
    }

    #[test]
    fn dto_serializes_states_as_lowercase_strings() {
        let cases = [
            (DeviceState::Connected, "connected"),
            (DeviceState::Unauthorized, "unauthorized"),
            (DeviceState::Offline, "offline"),
        ];

        for (state, expected) in cases {
            let value =
                serde_json::to_value(DeviceDto::from(device(state))).expect("dto should serialize");
            assert_eq!(value["state"], expected);
        }
    }

    #[test]
    fn replay_session_serializes_camel_case_fields() {
        let dto = ReplaySessionDto {
            ws_url: "ws://127.0.0.1:40000/stream?token=abcd".to_owned(),
            device: DeviceDto::from(device(DeviceState::Connected)),
        };
        let value = serde_json::to_value(&dto).expect("dto should serialize");

        assert_eq!(value["wsUrl"], "ws://127.0.0.1:40000/stream?token=abcd");
        assert!(value.get("ws_url").is_none());
        assert_eq!(value["device"]["serial"], "R58M12345");
        assert_eq!(value["device"]["state"], "connected");
        assert_eq!(value["device"]["transport"], "usb");
    }

    #[test]
    fn session_dto_serializes_camel_case_fields() {
        let dto = SessionDto {
            ws_url: "ws://127.0.0.1:40001/stream?token=1234".to_owned(),
            device: DeviceDto::from(device(DeviceState::Connected)),
        };
        let value = serde_json::to_value(&dto).expect("dto should serialize");

        assert_eq!(value["wsUrl"], "ws://127.0.0.1:40001/stream?token=1234");
        assert!(value.get("ws_url").is_none());
        assert_eq!(value["device"]["serial"], "R58M12345");
        assert_eq!(value["device"]["state"], "connected");
        assert_eq!(value["device"]["transport"], "usb");
    }

    #[test]
    fn replay_session_dto_is_the_live_session_dto() {
        let session = SessionDto {
            ws_url: "ws://127.0.0.1:40002/stream?token=5678".to_owned(),
            device: DeviceDto::from(device(DeviceState::Connected)),
        };
        let replay: ReplaySessionDto = session.clone();

        assert_eq!(
            serde_json::to_value(&session).expect("session should serialize"),
            serde_json::to_value(&replay).expect("replay should serialize")
        );
    }

    #[test]
    fn dto_keeps_missing_details_as_null() {
        let device = Device::new(
            Serial::new("1234567890ABC").expect("valid serial"),
            "Pixel 8 Pro",
            DeviceState::Unauthorized,
            None,
            None,
            None,
        )
        .expect("valid device");

        let value = serde_json::to_value(DeviceDto::from(device)).expect("dto should serialize");
        assert!(value["androidVersion"].is_null());
        assert!(value["resolution"].is_null());
        assert!(value["batteryPercent"].is_null());
    }

    #[test]
    fn pointer_input_serializes_camel_case_fields() {
        let input = PointerInput {
            action: PointerAction::Down,
            x: 100,
            y: 200,
            screen_width: 1080,
            screen_height: 2340,
            pressure: 1.0,
            buttons: 1,
            action_button: 1,
        };
        let value = serde_json::to_value(&input).expect("pointer should serialize");

        assert_eq!(value["action"], "down");
        assert_eq!(value["x"], 100);
        assert_eq!(value["y"], 200);
        assert_eq!(value["screenWidth"], 1080);
        assert_eq!(value["screenHeight"], 2340);
        assert_eq!(value["pressure"], 1.0);
        assert_eq!(value["buttons"], 1);
        assert_eq!(value["actionButton"], 1);
        assert!(value.get("screen_width").is_none());
        assert!(value.get("action_button").is_none());
    }

    #[test]
    fn pointer_input_deserializes_from_camel_case_payload() {
        let input: PointerInput = serde_json::from_value(serde_json::json!({
            "action": "move",
            "x": 10,
            "y": 20,
            "screenWidth": 1080,
            "screenHeight": 2340,
            "pressure": 0.5,
            "buttons": 1,
            "actionButton": 0
        }))
        .expect("pointer should deserialize");

        assert_eq!(
            input,
            PointerInput {
                action: PointerAction::Move,
                x: 10,
                y: 20,
                screen_width: 1080,
                screen_height: 2340,
                pressure: 0.5,
                buttons: 1,
                action_button: 0,
            }
        );
    }

    #[test]
    fn scroll_input_serializes_camel_case_fields() {
        let input = ScrollInput {
            x: 540,
            y: 1200,
            screen_width: 1080,
            screen_height: 2340,
            h_scroll: 0.0,
            v_scroll: -1.0,
            buttons: 0,
        };
        let value = serde_json::to_value(&input).expect("scroll should serialize");

        assert_eq!(value["screenWidth"], 1080);
        assert_eq!(value["screenHeight"], 2340);
        assert_eq!(value["hScroll"], 0.0);
        assert_eq!(value["vScroll"], -1.0);
        assert_eq!(value["buttons"], 0);
        assert!(value.get("h_scroll").is_none());
        assert!(value.get("v_scroll").is_none());

        let decoded: ScrollInput =
            serde_json::from_value(value).expect("scroll should deserialize back");
        assert_eq!(decoded, input);
    }

    #[test]
    fn key_input_serializes_camel_case_fields() {
        let input = KeyInput {
            action: KeyAction::Down,
            keycode: 4,
            repeat: 0,
            meta_state: 0x1000,
        };
        let value = serde_json::to_value(&input).expect("key should serialize");

        assert_eq!(value["action"], "down");
        assert_eq!(value["keycode"], 4);
        assert_eq!(value["repeat"], 0);
        assert_eq!(value["metaState"], 0x1000);
        assert!(value.get("meta_state").is_none());

        let decoded: KeyInput = serde_json::from_value(value).expect("key should deserialize back");
        assert_eq!(decoded, input);
    }

    #[test]
    fn input_actions_serialize_as_lowercase_strings() {
        let pointer_cases = [
            (PointerAction::Down, "down"),
            (PointerAction::Move, "move"),
            (PointerAction::Up, "up"),
        ];
        for (action, expected) in pointer_cases {
            assert_eq!(
                serde_json::to_value(action).expect("action should serialize"),
                serde_json::json!(expected)
            );
        }

        let key_cases = [(KeyAction::Down, "down"), (KeyAction::Up, "up")];
        for (action, expected) in key_cases {
            assert_eq!(
                serde_json::to_value(action).expect("action should serialize"),
                serde_json::json!(expected)
            );
        }
    }
}
