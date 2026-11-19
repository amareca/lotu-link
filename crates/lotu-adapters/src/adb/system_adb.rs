use std::ffi::{OsStr, OsString};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use lotu_app::ports::device_catalog::{CatalogError, DeviceCatalog};
use lotu_domain::{Device, DeviceState, Resolution, Serial};
use tokio::process::{Child, Command};
use tokio::time::timeout;

/// Budget for every ADB invocation (listing and each per-device probe).
pub const ADB_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Budget for pushing the scrcpy server jar (`docs/m1-contract.md` 3.1.1).
pub const PUSH_TIMEOUT: Duration = Duration::from_secs(10);

/// Budget for `adb forward` and `adb forward --remove` (3.1.3 and 4.5).
pub const FORWARD_TIMEOUT: Duration = Duration::from_secs(5);

/// Budget for `adb shell` used by the session cleanup (`rm -f`, 4.5 step 4).
pub const SESSION_SHELL_TIMEOUT: Duration = Duration::from_secs(2);

/// ADB-backed [`DeviceCatalog`].
#[derive(Debug, Clone)]
pub struct SystemAdb {
    binary: PathBuf,
    server_port: u16,
}

impl SystemAdb {
    pub fn new(binary: PathBuf, server_port: u16) -> Self {
        Self {
            binary,
            server_port,
        }
    }

    fn command<I, S>(&self, serial: Option<&Serial>, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(&self.binary);
        command.arg("-P").arg(self.server_port.to_string());
        if let Some(serial) = serial {
            command.arg("-s").arg(serial.as_str());
        }
        command.args(args);
        command.kill_on_drop(true);
        command.stdin(Stdio::null());
        command
    }

    async fn run(&self, serial: Option<&Serial>, args: &[&str]) -> Result<String, AdbFailure> {
        self.run_with_timeout(serial, args.iter().copied(), ADB_COMMAND_TIMEOUT)
            .await
    }

    async fn run_with_timeout<I, S>(
        &self,
        serial: Option<&Serial>,
        args: I,
        budget: Duration,
    ) -> Result<String, AdbFailure>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command(serial, args);

        match timeout(budget, command.output()).await {
            Err(_) => Err(AdbFailure::Timeout),
            Ok(Err(error)) => Err(AdbFailure::from_spawn(error)),
            Ok(Ok(output)) if !output.status.success() => Err(AdbFailure::from_output(&output)),
            Ok(Ok(output)) => String::from_utf8(output.stdout)
                .map_err(|_| AdbFailure::Failed("adb produced non UTF-8 output".to_owned())),
        }
    }

    /// Runs `adb <args>` against `serial` with a session budget and maps every
    /// failure to the shared [`CatalogError`].
    async fn run_session<I, S>(
        &self,
        serial: &Serial,
        args: I,
        budget: Duration,
    ) -> Result<String, CatalogError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_with_timeout(Some(serial), args, budget)
            .await
            .map_err(CatalogError::from)
    }

    /// `adb push <local> <remote>`; overwrites a previous copy (idempotent).
    pub async fn push(
        &self,
        serial: &Serial,
        local_path: &Path,
        remote_path: &str,
    ) -> Result<(), CatalogError> {
        self.run_session(serial, push_args(local_path, remote_path), PUSH_TIMEOUT)
            .await
            .map(|_| ())
    }

    /// `adb forward <local_spec> <remote_spec>`.
    ///
    /// The specs are built by the scrcpy adapter (`tcp:<port>` and
    /// `localabstract:scrcpy_<scid>`), which owns the tunnel naming.
    pub async fn forward(
        &self,
        serial: &Serial,
        local_spec: &str,
        remote_spec: &str,
    ) -> Result<(), CatalogError> {
        self.run_session(
            serial,
            forward_args(local_spec, remote_spec),
            FORWARD_TIMEOUT,
        )
        .await
        .map(|_| ())
    }

    /// `adb forward --remove <local_spec>`.
    pub async fn remove_forward(
        &self,
        serial: &Serial,
        local_spec: &str,
    ) -> Result<(), CatalogError> {
        self.run_session(serial, remove_forward_args(local_spec), FORWARD_TIMEOUT)
            .await
            .map(|_| ())
    }

    /// Starts the scrcpy server without waiting for it: `adb shell <args>`.
    ///
    /// `args` are the arguments that follow `adb shell` (see
    /// `scrcpy::session::app_process_args`). The returned [`Child`] keeps running in
    /// the background; its stdout/stderr are piped for diagnostics and it is
    /// killed on drop. Spawning is never bounded by a timeout because the
    /// process is meant to outlive the call.
    pub fn spawn_server(&self, serial: &Serial, args: &[String]) -> Result<Child, CatalogError> {
        self.server_command(serial, args)
            .spawn()
            .map_err(|error| CatalogError::from(AdbFailure::from_spawn(error)))
    }

    fn server_command(&self, serial: &Serial, args: &[String]) -> Command {
        let mut command = self.command(
            Some(serial),
            std::iter::once(OsString::from("shell"))
                .chain(args.iter().map(|arg| OsString::from(arg.as_str()))),
        );
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command
    }

    /// `adb shell <args>`: the arguments are the command and its own arguments.
    pub async fn shell(&self, serial: &Serial, args: &[&str]) -> Result<String, CatalogError> {
        self.run_session(serial, shell_invocation(args), SESSION_SHELL_TIMEOUT)
            .await
    }

    async fn android_version(&self, serial: &Serial) -> Option<String> {
        let output = self
            .run(
                Some(serial),
                &["shell", "getprop", "ro.build.version.release"],
            )
            .await
            .ok()?;
        parse_android_version(&output)
    }

    async fn screen_resolution(&self, serial: &Serial) -> Option<Resolution> {
        let output = self
            .run(Some(serial), &["shell", "wm", "size"])
            .await
            .ok()?;
        parse_wm_size(&output)
    }

    async fn battery_percent(&self, serial: &Serial) -> Option<u8> {
        let output = self
            .run(Some(serial), &["shell", "dumpsys", "battery"])
            .await
            .ok()?;
        parse_battery_level(&output)
    }
}

#[async_trait]
impl DeviceCatalog for SystemAdb {
    async fn list(&self) -> Result<Vec<Device>, CatalogError> {
        let output = self.run(None, &["devices", "-l"]).await?;
        let entries = parse_devices_output(&output)?;

        let mut devices = Vec::with_capacity(entries.len());
        for entry in entries {
            // Optional fields degrade to `None`; a probe failure never fails the
            // whole listing.
            let (android_version, resolution, battery_percent) =
                if entry.state == DeviceState::Connected {
                    tokio::join!(
                        self.android_version(&entry.serial),
                        self.screen_resolution(&entry.serial),
                        self.battery_percent(&entry.serial),
                    )
                } else {
                    (None, None, None)
                };

            let device = Device::new(
                entry.serial,
                entry.name,
                entry.state,
                android_version,
                resolution,
                battery_percent,
            )
            .map_err(|error| CatalogError::Failed(format!("invalid adb device entry: {error}")))?;

            devices.push(device);
        }

        Ok(devices)
    }
}

#[derive(Debug, thiserror::Error)]
enum AdbFailure {
    #[error("adb is not available: {0}")]
    Unavailable(String),
    #[error("adb did not answer in time")]
    Timeout,
    #[error("{0}")]
    Failed(String),
}

impl AdbFailure {
    fn from_spawn(error: std::io::Error) -> Self {
        match error.kind() {
            ErrorKind::NotFound => Self::Unavailable(error.to_string()),
            _ => Self::Failed(error.to_string()),
        }
    }

    fn from_output(output: &std::process::Output) -> Self {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            Self::Failed(format!("adb exited with status {}", output.status))
        } else {
            Self::Failed(format!(
                "adb exited with status {}: {stderr}",
                output.status
            ))
        }
    }
}

impl From<AdbFailure> for CatalogError {
    fn from(failure: AdbFailure) -> Self {
        match failure {
            AdbFailure::Unavailable(message) => Self::Unavailable(message),
            AdbFailure::Timeout => Self::Timeout,
            AdbFailure::Failed(message) => Self::Failed(message),
        }
    }
}

#[derive(Debug)]
struct AdbDeviceEntry {
    serial: Serial,
    state: DeviceState,
    name: String,
}

/// Arguments of `adb push` (`adb` and the `-P`/`-s` prefix are added by
/// [`SystemAdb::command`]).
fn push_args(local_path: &Path, remote_path: &str) -> [OsString; 3] {
    [
        OsString::from("push"),
        local_path.as_os_str().to_os_string(),
        OsString::from(remote_path),
    ]
}

/// Arguments of `adb forward`.
fn forward_args(local_spec: &str, remote_spec: &str) -> [String; 3] {
    [
        String::from("forward"),
        local_spec.to_owned(),
        remote_spec.to_owned(),
    ]
}

/// Arguments of `adb forward --remove`.
fn remove_forward_args(local_spec: &str) -> [String; 3] {
    [
        String::from("forward"),
        String::from("--remove"),
        local_spec.to_owned(),
    ]
}

/// Arguments of `adb shell <command...>`.
fn shell_invocation<'a>(args: &'a [&'a str]) -> Vec<&'a str> {
    let mut full = Vec::with_capacity(args.len() + 1);
    full.push("shell");
    full.extend_from_slice(args);
    full
}

fn parse_devices_output(output: &str) -> Result<Vec<AdbDeviceEntry>, CatalogError> {
    let mut entries = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line == "List of devices attached" {
            continue;
        }

        let mut fields = line.split_whitespace();
        let serial_raw = fields.next().ok_or_else(|| malformed_line(line))?;
        let state_raw = fields.next().ok_or_else(|| malformed_line(line))?;

        // The MVP only handles USB devices: emulators and TCP serials are out.
        if serial_raw.starts_with("emulator-") || serial_raw.contains(':') {
            continue;
        }

        let Some(state) = parse_state(state_raw) else {
            // States the MVP cannot represent (bootloader, recovery, ...) are
            // ignored instead of failing the whole listing.
            continue;
        };

        let serial = Serial::new(serial_raw).map_err(|error| {
            CatalogError::Failed(format!("unexpected adb serial {serial_raw:?}: {error}"))
        })?;

        let mut model = None;
        let mut device = None;
        for field in fields {
            if let Some(value) = field.strip_prefix("model:") {
                model = Some(value);
            } else if let Some(value) = field.strip_prefix("device:") {
                device = Some(value);
            }
        }

        entries.push(AdbDeviceEntry {
            serial: serial.clone(),
            state,
            name: device_name(model, device, &serial),
        });
    }

    Ok(entries)
}

fn malformed_line(line: &str) -> CatalogError {
    CatalogError::Failed(format!("unexpected adb devices output line: {line:?}"))
}

fn parse_state(raw: &str) -> Option<DeviceState> {
    match raw {
        "device" => Some(DeviceState::Connected),
        "unauthorized" => Some(DeviceState::Unauthorized),
        "offline" => Some(DeviceState::Offline),
        _ => None,
    }
}

fn device_name(model: Option<&str>, device: Option<&str>, serial: &Serial) -> String {
    model
        .filter(|value| !value.is_empty())
        .or_else(|| device.filter(|value| !value.is_empty()))
        .map(|value| value.replace('_', " "))
        .unwrap_or_else(|| serial.as_str().to_owned())
}

fn parse_android_version(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

fn parse_wm_size(output: &str) -> Option<Resolution> {
    let mut physical = None;
    let mut overridden = None;

    for line in output.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Override size:") {
            overridden = parse_size(value);
        } else if let Some(value) = line.strip_prefix("Physical size:") {
            physical = parse_size(value);
        }
    }

    overridden.or(physical)
}

fn parse_size(raw: &str) -> Option<Resolution> {
    let (width, height) = raw.trim().split_once(['x', 'X'])?;
    let width = width.trim().parse::<u32>().ok()?;
    let height = height.trim().parse::<u32>().ok()?;
    Resolution::new(width, height).ok()
}

fn parse_battery_level(output: &str) -> Option<u8> {
    for line in output.lines() {
        if let Some(value) = line.trim().strip_prefix("level:") {
            if let Ok(level) = value.trim().parse::<u8>() {
                return (level <= 100).then_some(level);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADB_DEVICES_FIXTURE: &str = include_str!("../../tests/fixtures/adb-devices.txt");
    const WM_SIZE_OVERRIDE_FIXTURE: &str =
        include_str!("../../tests/fixtures/wm-size-override.txt");
    const WM_SIZE_PHYSICAL_FIXTURE: &str =
        include_str!("../../tests/fixtures/wm-size-physical.txt");
    const DUMPSYS_BATTERY_FIXTURE: &str = include_str!("../../tests/fixtures/dumpsys-battery.txt");

    fn serial(raw: &str) -> Serial {
        Serial::new(raw).expect("test serial should be valid")
    }

    #[test]
    fn parses_usb_devices_and_excludes_emulators_and_tcp() {
        let entries = parse_devices_output(ADB_DEVICES_FIXTURE).expect("fixture should parse");

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].serial.as_str(), "R58M12345");
        assert_eq!(entries[0].state, DeviceState::Connected);
        assert_eq!(entries[0].name, "Pixel 8 Pro");
        assert_eq!(entries[1].serial.as_str(), "1234567890ABC");
        assert_eq!(entries[1].state, DeviceState::Unauthorized);
        assert_eq!(entries[2].serial.as_str(), "ZY22GABC");
        assert_eq!(entries[2].state, DeviceState::Offline);
    }

    #[test]
    fn model_underscores_become_spaces() {
        let entries = parse_devices_output(ADB_DEVICES_FIXTURE).expect("fixture should parse");

        assert_eq!(entries[0].name, "Pixel 8 Pro");
        assert!(!entries[0].name.contains('_'));
    }

    #[test]
    fn falls_back_to_device_property_when_model_is_missing() {
        let name = device_name(None, Some("beyond1"), &serial("R58M12345"));
        assert_eq!(name, "beyond1");

        let name = device_name(None, Some("pixel_8_pro"), &serial("R58M12345"));
        assert_eq!(name, "pixel 8 pro");
    }

    #[test]
    fn falls_back_to_serial_when_model_and_device_are_missing() {
        let name = device_name(None, None, &serial("R58M12345"));
        assert_eq!(name, "R58M12345");
    }

    #[test]
    fn ignores_lines_with_unknown_connection_states() {
        let output = "R58M12345 bootloader usb:1-1 transport_id:1\n";
        let entries = parse_devices_output(output).expect("unknown states are ignored");
        assert!(entries.is_empty());
    }

    #[test]
    fn rejects_malformed_device_lines() {
        for output in ["only-one-field\n", "another-broken-line"] {
            let error = parse_devices_output(output).expect_err("line should be rejected");
            assert!(matches!(error, CatalogError::Failed(_)));
        }
    }

    #[test]
    fn parses_wm_size_with_override() {
        let resolution = parse_wm_size(WM_SIZE_OVERRIDE_FIXTURE).expect("override should win");
        assert_eq!(resolution.width, 720);
        assert_eq!(resolution.height, 1600);
    }

    #[test]
    fn parses_wm_size_without_override() {
        let resolution = parse_wm_size(WM_SIZE_PHYSICAL_FIXTURE).expect("physical size");
        assert_eq!(resolution.width, 1080);
        assert_eq!(resolution.height, 2400);
    }

    #[test]
    fn ignores_unparseable_wm_size_output() {
        assert!(parse_wm_size("").is_none());
        assert!(parse_wm_size("Physical size: 0x2400").is_none());
        assert!(parse_wm_size("Override size: hdpi").is_none());
        assert!(parse_wm_size("WARNING: nothing here").is_none());
    }

    #[test]
    fn parses_battery_level() {
        assert_eq!(parse_battery_level(DUMPSYS_BATTERY_FIXTURE), Some(85));
    }

    #[test]
    fn rejects_out_of_range_or_missing_battery_levels() {
        assert_eq!(parse_battery_level("  level: 300\n"), None);
        assert_eq!(parse_battery_level("  level: not-a-number\n"), None);
        assert_eq!(parse_battery_level("  AC powered: false\n"), None);
    }

    #[test]
    fn parses_android_version() {
        assert_eq!(parse_android_version("15\n"), Some("15".to_owned()));
        assert_eq!(parse_android_version("\n  15  \n"), Some("15".to_owned()));
        assert_eq!(parse_android_version(""), None);
        assert_eq!(parse_android_version("\n \n"), None);
    }

    #[test]
    fn command_always_pins_server_port() {
        let adb = SystemAdb::new(PathBuf::from("adb"), 5038);
        let command = adb.command(None, ["devices", "-l"]);
        let command = command.as_std();

        assert_eq!(command.get_program(), "adb");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args, ["-P", "5038", "devices", "-l"]);
    }

    #[test]
    fn command_targets_the_requested_device() {
        let adb = SystemAdb::new(PathBuf::from("adb"), 5039);
        let serial = serial("R58M12345");
        let command = adb.command(Some(&serial), ["shell", "wm", "size"]);
        let command = command.as_std();

        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            ["-P", "5039", "-s", "R58M12345", "shell", "wm", "size"]
        );
    }

    fn command_args(command: &std::process::Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn session_timeouts_follow_the_contract() {
        assert_eq!(PUSH_TIMEOUT, Duration::from_secs(10));
        assert_eq!(FORWARD_TIMEOUT, Duration::from_secs(5));
        assert_eq!(SESSION_SHELL_TIMEOUT, Duration::from_secs(2));
    }

    #[test]
    fn push_args_keep_the_local_path_and_the_remote_target() {
        let args: Vec<String> = push_args(
            Path::new(r"C:\vendor\scrcpy-server.jar"),
            "/data/local/tmp/lotu-link-server.jar",
        )
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

        assert_eq!(args[0], "push");
        assert_eq!(args[1], r"C:\vendor\scrcpy-server.jar");
        assert_eq!(args[2], "/data/local/tmp/lotu-link-server.jar");
    }

    #[test]
    fn forward_args_match_the_tunnel_specs() {
        assert_eq!(
            forward_args("tcp:27183", "localabstract:scrcpy_0000abcd"),
            ["forward", "tcp:27183", "localabstract:scrcpy_0000abcd"]
        );
    }

    #[test]
    fn remove_forward_args_use_the_remove_flag() {
        assert_eq!(
            remove_forward_args("tcp:27183"),
            ["forward", "--remove", "tcp:27183"]
        );
    }

    #[test]
    fn shell_invocation_prefixes_the_shell_subcommand() {
        assert_eq!(
            shell_invocation(&["rm", "-f", "/data/local/tmp/lotu-link-server.jar"]),
            ["shell", "rm", "-f", "/data/local/tmp/lotu-link-server.jar"]
        );
        assert_eq!(shell_invocation(&[]), ["shell"]);
    }

    #[test]
    fn server_command_pins_port_serial_and_shell() {
        let adb = SystemAdb::new(PathBuf::from("adb"), 5038);
        let server = vec![
            "CLASSPATH=/data/local/tmp/lotu-link-server.jar".to_owned(),
            "app_process".to_owned(),
        ];

        let command = adb.server_command(&serial("R58M12345"), &server);

        assert_eq!(
            command_args(command.as_std()),
            [
                "-P",
                "5038",
                "-s",
                "R58M12345",
                "shell",
                "CLASSPATH=/data/local/tmp/lotu-link-server.jar",
                "app_process",
            ]
        );
    }

    #[test]
    fn spawn_server_maps_a_missing_binary_to_a_catalog_error() {
        let adb = SystemAdb::new(PathBuf::from("lotu-adb-does-not-exist-xyz"), 5038);
        let error = adb
            .spawn_server(&serial("R58M12345"), &["app_process".to_owned()])
            .expect_err("the binary does not exist");

        assert!(matches!(error, CatalogError::Unavailable(_)));
    }
}
