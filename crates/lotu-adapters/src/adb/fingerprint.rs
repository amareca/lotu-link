//! ADB RSA fingerprint: the value Android shows in the authorization dialog.
//!
//! Algorithm from AOSP `AdbDebuggingManager.getFingerprints()`: take the first
//! whitespace-delimited token of the public key, Base64-decode it, MD5 the
//! bytes and print 16 uppercase hex pairs separated by colons.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use lotu_app::ports::{FingerprintError, FingerprintSource};
use md5::Digest;

use super::SystemAdb;

/// Resolves the ADB private key path using ADB's own precedence.
pub fn adb_key_path(
    sdk_home: Option<OsString>,
    user_profile: Option<OsString>,
    home: Option<OsString>,
) -> PathBuf {
    let base = non_empty(sdk_home)
        .or_else(|| non_empty(user_profile))
        .or_else(|| non_empty(home))
        .map(PathBuf::from)
        .unwrap_or_default();
    base.join(".android").join("adbkey")
}

fn non_empty(value: Option<OsString>) -> Option<OsString> {
    value.filter(|value| !value.is_empty())
}

/// Computes the fingerprint from a public key line (`<base64> <comment>`).
pub fn fingerprint_from_public_key(text: &str) -> Result<String, FingerprintError> {
    let token = text
        .split_whitespace()
        .next()
        .filter(|token| !token.is_empty())
        .ok_or(FingerprintError::Unreadable)?;
    let bytes = STANDARD
        .decode(token)
        .map_err(|_| FingerprintError::Unreadable)?;
    let digest = md5::Md5::digest(bytes);
    Ok(digest
        .as_slice()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":"))
}

/// Reads the current fingerprint through the same ADB binary the app uses.
pub struct AdbFingerprintSource {
    adb: Arc<SystemAdb>,
}

impl AdbFingerprintSource {
    pub fn new(adb: Arc<SystemAdb>) -> Self {
        Self { adb }
    }
}

#[async_trait]
impl FingerprintSource for AdbFingerprintSource {
    async fn current(&self) -> Result<String, FingerprintError> {
        let key = adb_key_path(
            std::env::var_os("ANDROID_SDK_HOME"),
            std::env::var_os("USERPROFILE"),
            std::env::var_os("HOME"),
        );

        if let Ok(text) = self.adb.pubkey(&key).await {
            return fingerprint_from_public_key(&text);
        }

        // Older ADB versions may leave a `.pub` file behind.
        let public = key.with_extension("pub");
        if let Ok(text) = tokio::fs::read_to_string(&public).await {
            if let Ok(fingerprint) = fingerprint_from_public_key(&text) {
                return Ok(fingerprint);
            }
        }
        Err(FingerprintError::KeyUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Public example key from Stack Overflow; its decoded form is 524 bytes.
    const EXAMPLE_PUBLIC_KEY: &str = "QAAAAOlEXtSnLMF3igx8NMi7u8+LeD7BoVC+v2bvBJnvVsaJ31QtwEzbicob8mlLxEhbGSKdaoXIwwAsWR+7FzlSUW57G9vuqpJDGJ7iEG4+5uYs7KarhiRF3K+hUX6PDIF7gEo/0TgglxvNXmTkfV9zZb3VxmgV66z68VeBXK46kM7MffKiHyu7P3Jdtdptm2p7jU7XwvfgWH6a0rrrGzUWONEfteh6ruHIxP1Z3CdxYVZ0YF9uHWsweQzgf7N2RG0g4cxNJDLs0CXqaao7xS16tzaCYfn7cZQPIKCb057oo+jMWvgsh+8gY8qgQtI5EHBizd7cPp3S/rVbzN8gUCo4aSuIn9TfT6uJ0S3D3TPWbHXs9Y8nskhIOM1Hkgv3CAODfzwH+ZM3nWmFD77FMtiWo/hJrMRcH63yvX5pVPYnqQyHcdembEM1Fbxg/qWAVtLxNFJqoFKYXYHl9ktGcM+3Izwvea5fAebmbWIuezKYF6F49Y3dIPA+fWxunGkbehih7o0S9RoWIQIYByteF+b/EN2ntSpwfuhD8G9n6Bfaz4mEVTG82Lj8YeK6+CYyEirSCl4Al7SGsb66E74Fnt+v+NouQFpZxCrrefm7sYug11NHSNiDeYa8cnatQsla+Cfd91GmgKsu+ZDO8uF8UR7/r3MKEDohfMCrWdIiZ0w6Ad6hata1OgEAAQA=";

    #[test]
    fn matches_the_android_fingerprint_format() {
        let line = format!("{EXAMPLE_PUBLIC_KEY} unknown@unknown");
        assert_eq!(
            fingerprint_from_public_key(&line).expect("fingerprint"),
            "14:DA:05:D3:8E:83:58:34:59:60:A9:2F:75:51:47:3D"
        );
    }

    #[test]
    fn rejects_malformed_public_keys() {
        assert!(fingerprint_from_public_key("").is_err());
        assert!(fingerprint_from_public_key("not base64! ignored").is_err());
    }

    #[test]
    fn resolves_the_key_path_with_adb_precedence() {
        assert_eq!(
            adb_key_path(
                Some(OsString::from("sdk")),
                Some(OsString::from("profile")),
                Some(OsString::from("home")),
            ),
            PathBuf::from("sdk/.android/adbkey")
        );
        assert_eq!(
            adb_key_path(None, Some(OsString::from("profile")), None),
            PathBuf::from("profile/.android/adbkey")
        );
        assert_eq!(
            adb_key_path(None, None, Some(OsString::from("home"))),
            PathBuf::from("home/.android/adbkey")
        );
    }
}
