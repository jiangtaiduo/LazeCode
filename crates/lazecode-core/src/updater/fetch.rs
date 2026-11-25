//! Fetch helpers: bounded reads, local/remote sources and platform checks.

use super::*;

/// Read a manifest from HTTP/HTTPS, a `file://` URL or a filesystem path,
/// refusing bodies larger than `max_bytes`.
pub fn read_limited(location: &str, max_bytes: u64) -> Result<Vec<u8>, UpdateError> {
    let (mut reader, _length) = open_source(location)?;
    let mut data = Vec::new();
    reader
        .by_ref()
        .take(max_bytes + 1)
        .read_to_end(&mut data)
        .map_err(io_error)?;
    if data.len() as u64 > max_bytes {
        return Err(UpdateError::TooLarge(max_bytes));
    }
    Ok(data)
}

pub(super) fn open_source(location: &str) -> Result<(Box<dyn Read>, Option<u64>), UpdateError> {
    let location = location.trim();
    if location.is_empty() {
        return Err(UpdateError::EndpointMissing);
    }
    if let Some(path) = local_path(location) {
        let meta = std::fs::metadata(&path).map_err(io_error)?;
        let file = std::fs::File::open(&path).map_err(io_error)?;
        return Ok((Box::new(file), Some(meta.len())));
    }
    if !location.starts_with("http://") && !location.starts_with("https://") {
        return Err(UpdateError::Http(format!(
            "unsupported update source {location:?} (expected https://, http://, file:// or a path)"
        )));
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(concat!("LazeCode/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| UpdateError::Http(err.to_string()))?;
    let response = client
        .get(location)
        .send()
        .map_err(|err| UpdateError::Http(err.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(UpdateError::Http(format!(
            "{location} responded with HTTP {status}"
        )));
    }
    let length = response.content_length();
    Ok((Box::new(response), length))
}

fn local_path(location: &str) -> Option<PathBuf> {
    if let Some(rest) = location.strip_prefix("file://") {
        return Some(PathBuf::from(rest));
    }
    if !location.contains("://") {
        return Some(PathBuf::from(location));
    }
    None
}

pub(super) fn io_error(err: std::io::Error) -> UpdateError {
    UpdateError::Io(err.to_string())
}

pub(super) fn ensure_platform(manifest: &Manifest, platform: &Platform) -> Result<(), UpdateError> {
    let required_arch = manifest.arch.as_deref().map(str::trim).unwrap_or("");
    if !required_arch.is_empty() && normalise_arch(required_arch) != normalise_arch(&platform.arch)
    {
        return Err(UpdateError::UnsupportedPlatform {
            required: format!("arch {required_arch}"),
            actual: format!("arch {}", platform.arch),
        });
    }
    let min_os = manifest.min_os.as_deref().map(str::trim).unwrap_or("");
    let Some(current) = platform.os_version.as_deref() else {
        return Ok(());
    };
    if min_os.is_empty() || compare_versions(min_os, current)? != Ordering::Greater {
        return Ok(());
    }
    Err(UpdateError::UnsupportedPlatform {
        required: format!("{} {min_os}", platform.os),
        actual: format!("{} {current}", platform.os),
    })
}

pub(super) fn normalise_arch(arch: &str) -> String {
    match arch.trim().to_ascii_lowercase().as_str() {
        "aarch64" | "arm64" | "arm64e" => "aarch64".to_string(),
        "x86_64" | "x64" | "amd64" => "x86_64".to_string(),
        "x86" | "i386" | "i686" => "x86".to_string(),
        other => other.to_string(),
    }
}

#[cfg(target_os = "macos")]
pub(super) fn current_os_version() -> Option<String> {
    let output = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!version.is_empty()).then_some(version)
}

#[cfg(target_os = "linux")]
pub(super) fn current_os_version() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    text.lines().find_map(|line| {
        line.strip_prefix("VERSION_ID=")
            .map(|value| value.trim_matches('"').trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(super) fn current_os_version() -> Option<String> {
    None
}
