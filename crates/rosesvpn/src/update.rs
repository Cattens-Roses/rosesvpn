use anyhow::{Context, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

const MANIFEST_URL: &str =
    "https://github.com/Cattens-Roses/rosesvpn/releases/download/continuous/update-manifest.json";
const RELEASE_ASSET_PREFIX: &str =
    "https://github.com/Cattens-Roses/rosesvpn/releases/download/continuous/";

#[derive(Clone, Debug, Deserialize)]
pub struct UpdateAsset {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct AvailableUpdate {
    pub version: String,
    pub asset: UpdateAsset,
}

#[derive(Deserialize)]
struct Manifest {
    product: String,
    version: String,
    platforms: std::collections::HashMap<String, PlatformAssets>,
}

#[derive(Deserialize)]
struct PlatformAssets {
    installer: UpdateAsset,
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const PLATFORM: &str = "windows-x64";
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PLATFORM: &str = "linux-x64";

#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64")
)))]
const PLATFORM: &str = "unsupported";

pub fn check_for_update(current_version: &str) -> anyhow::Result<Option<AvailableUpdate>> {
    if std::env::var_os("ROSESVPN_DISABLE_UPDATE").is_some() {
        return Ok(None);
    }
    anyhow::ensure!(
        PLATFORM != "unsupported",
        "automatic updates are not available for this platform"
    );

    let output = curl()
        .args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
            "20",
            MANIFEST_URL,
        ])
        .output()
        .context("Could not start the system HTTPS client (curl)")?;
    ensure!(
        output.status.success(),
        "Could not download the update manifest: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    ensure!(
        output.stdout.len() <= 128 * 1024,
        "Update manifest is unexpectedly large"
    );

    let manifest: Manifest =
        serde_json::from_slice(&output.stdout).context("Invalid update manifest")?;
    ensure!(
        manifest.product == "RosesVPN",
        "Update manifest belongs to another product"
    );
    if manifest.version == current_version {
        return Ok(None);
    }

    let platform = manifest
        .platforms
        .get(PLATFORM)
        .context("No release is available for this platform")?;
    validate_asset(&platform.installer)?;
    Ok(Some(AvailableUpdate {
        version: manifest.version,
        asset: platform.installer.clone(),
    }))
}

fn validate_asset(asset: &UpdateAsset) -> anyhow::Result<()> {
    ensure!(
        asset.url.starts_with(RELEASE_ASSET_PREFIX),
        "Update asset URL is outside the official RosesVPN release channel"
    );
    ensure!(
        asset.name == asset.url[RELEASE_ASSET_PREFIX.len()..],
        "Update asset name does not match its URL"
    );
    ensure!(
        asset.sha256.len() == 64 && asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Update asset has an invalid SHA-256 hash"
    );
    ensure!(
        asset.size > 0 && asset.size <= 512 * 1024 * 1024,
        "Update asset size is invalid"
    );
    #[cfg(windows)]
    ensure!(
        asset.name == "RosesVPN-Setup.exe",
        "Unexpected Windows update installer"
    );
    #[cfg(target_os = "linux")]
    ensure!(
        asset.name.starts_with("rosesvpn_") && asset.name.ends_with("_amd64.deb"),
        "Unexpected Linux update package"
    );
    Ok(())
}

fn curl() -> Command {
    #[cfg(windows)]
    let command = Command::new("curl.exe");
    #[cfg(not(windows))]
    let command = Command::new("curl");
    command
}

pub fn download_and_launch(asset: &UpdateAsset) -> anyhow::Result<()> {
    validate_asset(asset)?;
    let filename = if cfg!(windows) {
        format!("rosesvpn-update-{}.exe", std::process::id())
    } else {
        format!("rosesvpn-update-{}.deb", std::process::id())
    };
    let path = std::env::temp_dir().join(filename);
    let output = curl()
        .args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
            "180",
            "--output",
        ])
        .arg(&path)
        .arg(&asset.url)
        .output()
        .context("Could not start curl to download the update")?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&path);
        anyhow::bail!(
            "Download failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let verify = (|| -> anyhow::Result<()> {
        ensure!(
            std::fs::metadata(&path)?.len() == asset.size,
            "Downloaded update size does not match the manifest"
        );
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let digest = format!("{:x}", hasher.finalize());
        ensure!(
            digest.eq_ignore_ascii_case(&asset.sha256),
            "Downloaded update failed SHA-256 verification"
        );
        Ok(())
    })();
    if let Err(error) = verify {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }

    launch_installer(&path)?;
    Ok(())
}

#[cfg(windows)]
fn launch_installer(path: &PathBuf) -> anyhow::Result<()> {
    Command::new(path)
        .arg("/VERYSILENT")
        .arg("/SUPPRESSMSGBOXES")
        .arg("/CLOSEAPPLICATIONS")
        .arg("/NORESTART")
        .spawn()
        .context("Could not launch the RosesVPN setup program")?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn launch_installer(path: &PathBuf) -> anyhow::Result<()> {
    let executable = std::env::current_exe()?.canonicalize()?;
    ensure!(
        executable == PathBuf::from("/usr/bin/rosesvpn")
            || executable == PathBuf::from("/usr/local/bin/rosesvpn"),
        "Automatic Linux updates require the DEB-installed app. Download the portable archive from the release page instead."
    );
    Command::new("pkexec")
        .arg("/usr/bin/apt")
        .arg("install")
        .arg("-y")
        .arg(path)
        .spawn()
        .context("Could not launch the system package updater (pkexec)")?;
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn launch_installer(_path: &PathBuf) -> anyhow::Result<()> {
    anyhow::bail!("Automatic updates are not supported on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_update_urls_outside_official_release_channel() {
        let asset = UpdateAsset {
            name: "RosesVPN-Setup.exe".into(),
            url: "https://example.com/RosesVPN-Setup.exe".into(),
            sha256: "a".repeat(64),
            size: 1,
        };
        assert!(validate_asset(&asset).is_err());
    }

    #[test]
    fn rejects_invalid_hashes_and_empty_assets() {
        let asset = UpdateAsset {
            name: "RosesVPN-Setup.exe".into(),
            url: format!("{RELEASE_ASSET_PREFIX}RosesVPN-Setup.exe"),
            sha256: "not-a-hash".into(),
            size: 0,
        };
        assert!(validate_asset(&asset).is_err());
    }
}
