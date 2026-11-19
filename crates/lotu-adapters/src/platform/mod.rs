//! Platform-specific binary and configuration resolution.
//!
//! In M1 the ADB binary is provided by the system (or by `LOTU_ADB_PATH`); the
//! bundled platform-tools flow is deferred to M4. The scrcpy server jar is a
//! vendored artifact resolved here (see `docs/m1-contract.md` 2.5).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const DEFAULT_ADB_SERVER_PORT: u16 = 5038;

/// Vendored scrcpy server jar, relative to the workspace root.
pub const SCRCPY_SERVER_RELATIVE_PATH: &str = "src-tauri/resources/scrcpy-server.jar";

const ADB_PATH_ENV: &str = "LOTU_ADB_PATH";
const ADB_SERVER_PORT_ENV: &str = "LOTU_ADB_SERVER_PORT";
const SCRCPY_SERVER_PATH_ENV: &str = "LOTU_SCRCPY_SERVER_PATH";
const FALLBACK_ADB_BINARY: &str = "adb";

/// Failure of the vendored scrcpy server lookup.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VendorError {
    #[error(
        "scrcpy server not found at {path}: run `node scripts/fetch-vendor.mjs` (or set {env})"
    )]
    VendorMissing { path: PathBuf, env: &'static str },
}

/// Resolves the ADB binary: `LOTU_ADB_PATH` when set and non-empty, otherwise
/// `adb` from `PATH`.
pub fn adb_binary_path() -> PathBuf {
    adb_binary_from_env(std::env::var_os(ADB_PATH_ENV))
}

/// Resolves the private ADB server port: `LOTU_ADB_SERVER_PORT` when it parses
/// as a non-zero `u16`, otherwise [`DEFAULT_ADB_SERVER_PORT`].
pub fn adb_server_port() -> u16 {
    adb_server_port_from_env(std::env::var(ADB_SERVER_PORT_ENV).ok().as_deref())
}

/// Resolves the vendored scrcpy server jar.
///
/// `LOTU_SCRCPY_SERVER_PATH` wins when set and non-empty; otherwise the dev
/// default `<CARGO_MANIFEST_DIR>/../../src-tauri/resources/scrcpy-server.jar`
/// (the workspace root, since this crate lives in `crates/<crate>`) is used.
/// The path is returned even if the file does not exist; session code calls
/// [`require_scrcpy_server`] before touching ADB.
pub fn scrcpy_server_path() -> PathBuf {
    scrcpy_server_path_from(
        std::env::var_os(SCRCPY_SERVER_PATH_ENV),
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
}

/// Checks that `path` exists and is a regular file.
pub fn require_scrcpy_server(path: &Path) -> Result<&Path, VendorError> {
    if path.is_file() {
        Ok(path)
    } else {
        Err(VendorError::VendorMissing {
            path: path.to_path_buf(),
            env: SCRCPY_SERVER_PATH_ENV,
        })
    }
}

fn adb_binary_from_env(value: Option<OsString>) -> PathBuf {
    match value {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => PathBuf::from(FALLBACK_ADB_BINARY),
    }
}

fn adb_server_port_from_env(value: Option<&str>) -> u16 {
    value
        .and_then(|raw| raw.trim().parse::<u16>().ok())
        .filter(|port| *port != 0)
        .unwrap_or(DEFAULT_ADB_SERVER_PORT)
}

fn scrcpy_server_path_from(value: Option<OsString>, manifest_dir: &Path) -> PathBuf {
    match value {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        // The crate lives in `crates/<crate>`, so the workspace root is two
        // levels above its manifest directory. Resolving through `parent()`
        // keeps the returned path free of `..` segments.
        _ => workspace_root_from(manifest_dir).join(SCRCPY_SERVER_RELATIVE_PATH),
    }
}

fn workspace_root_from(manifest_dir: &Path) -> &Path {
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(manifest_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adb_binary_falls_back_to_path_lookup() {
        assert_eq!(adb_binary_from_env(None), PathBuf::from("adb"));
    }

    #[test]
    fn adb_binary_uses_override() {
        let explicit = PathBuf::from(r"C:\Android\platform-tools\adb.exe");
        assert_eq!(
            adb_binary_from_env(Some(explicit.clone().into_os_string())),
            explicit
        );
    }

    #[test]
    fn adb_binary_ignores_empty_override() {
        assert_eq!(
            adb_binary_from_env(Some(OsString::new())),
            PathBuf::from("adb")
        );
    }

    #[test]
    fn adb_server_port_defaults_to_5038() {
        assert_eq!(adb_server_port_from_env(None), 5038);
    }

    #[test]
    fn adb_server_port_accepts_valid_override() {
        assert_eq!(adb_server_port_from_env(Some("5039")), 5039);
        assert_eq!(adb_server_port_from_env(Some(" 5040 ")), 5040);
    }

    #[test]
    fn adb_server_port_rejects_invalid_values() {
        for raw in ["", "0", "not-a-port", "70000", "-1"] {
            assert_eq!(
                adb_server_port_from_env(Some(raw)),
                DEFAULT_ADB_SERVER_PORT,
                "{raw:?} should fall back to the default"
            );
        }
    }

    #[test]
    fn scrcpy_server_uses_the_override_when_present() {
        let explicit = PathBuf::from(r"D:\vendor\scrcpy-server.jar");
        assert_eq!(
            scrcpy_server_path_from(
                Some(explicit.clone().into_os_string()),
                Path::new(r"C:\workspace\lotu-link\crates\lotu-adapters"),
            ),
            explicit
        );
    }

    #[test]
    fn scrcpy_server_ignores_an_empty_override() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let expected = repo_root()
            .join("src-tauri")
            .join("resources")
            .join("scrcpy-server.jar");

        assert_eq!(
            scrcpy_server_path_from(Some(OsString::new()), manifest_dir),
            expected
        );
        assert_eq!(scrcpy_server_path_from(None, manifest_dir), expected);
    }

    #[test]
    fn scrcpy_server_default_resolves_the_committed_jar() {
        let path = scrcpy_server_path_from(None, Path::new(env!("CARGO_MANIFEST_DIR")));
        let expected = repo_root()
            .join("src-tauri")
            .join("resources")
            .join("scrcpy-server.jar");

        assert_eq!(path, expected);
        assert!(!path.to_string_lossy().contains(".."));

        // The committed artifact must exist, be a non-empty file and open as a
        // readable zip/jar.
        let metadata = std::fs::metadata(&path).expect("committed scrcpy jar metadata");
        assert!(metadata.is_file(), "{path:?} should be a regular file");
        assert!(metadata.len() > 0, "{path:?} should not be empty");

        let mut jar = std::fs::File::open(&path).expect("open committed scrcpy jar");
        let mut magic = [0_u8; 4];
        std::io::Read::read_exact(&mut jar, &mut magic).expect("read jar header");
        assert_eq!(&magic, b"PK\x03\x04", "{path:?} should be a zip/jar");

        assert_eq!(require_scrcpy_server(&path), Ok(path.as_path()));
    }

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("lotu-adapters lives in crates/<crate>")
    }

    #[test]
    fn require_scrcpy_server_accepts_an_existing_file() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert_eq!(require_scrcpy_server(&manifest), Ok(manifest.as_path()));
    }

    #[test]
    fn require_scrcpy_server_reports_a_typed_vendor_error() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/does-not-exist.jar");
        match require_scrcpy_server(&missing) {
            Err(VendorError::VendorMissing { path, env }) => {
                assert_eq!(path, missing);
                assert_eq!(env, "LOTU_SCRCPY_SERVER_PATH");
            }
            other => panic!("expected VendorMissing, got {other:?}"),
        }

        // A directory is not a usable jar either.
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(matches!(
            require_scrcpy_server(directory),
            Err(VendorError::VendorMissing { .. })
        ));
    }
}
