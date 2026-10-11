#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Command;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    WireGuard,
    OpenVpn,
}

impl Protocol {
    pub fn from_config(config: &str) -> anyhow::Result<Self> {
        if config.lines().any(|line| line.trim() == "[Interface]") {
            Ok(Self::WireGuard)
        } else if config.lines().any(|line| line.trim() == "client") && config.contains("<ca>") {
            Ok(Self::OpenVpn)
        } else {
            anyhow::bail!("Unsupported VPN configuration. Import a WireGuard .conf file.")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateEvent {
    Connect,
    Connected,
    Disconnect,
    Disconnected,
    Failure,
    Retry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VpnProfile {
    pub name: String,
    pub endpoint: String,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransferStats {
    pub received_bytes: u64,
    pub sent_bytes: u64,
}

/// Read WireGuard peer counters through the native kernel/control API.
/// This deliberately does not invoke `wg`, `wg-quick`, or the WireGuard GUI executable.
pub fn transfer_stats(config: &Path) -> anyhow::Result<TransferStats> {
    let record = fs::read_to_string(native_stats_path(config)?)?;
    let mut values = record.split_whitespace();
    let updated: u64 = values
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing stats timestamp"))?
        .parse()?;
    let received_bytes: u64 = values
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing received counter"))?
        .parse()?;
    let sent_bytes: u64 = values
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing sent counter"))?
        .parse()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    if now.saturating_sub(updated) > 5 {
        anyhow::bail!("Native tunnel statistics are stale");
    }
    Ok(TransferStats {
        received_bytes,
        sent_bytes,
    })
}

impl TunnelState {
    pub fn transition(self, event: StateEvent) -> anyhow::Result<Self> {
        match (self, event) {
            (Self::Disconnected, StateEvent::Connect) | (Self::Error, StateEvent::Retry) => {
                Ok(Self::Connecting)
            }
            (Self::Connecting, StateEvent::Connected) => Ok(Self::Connected),
            (Self::Connecting, StateEvent::Disconnect)
            | (Self::Connected, StateEvent::Disconnect) => Ok(Self::Disconnecting),
            (Self::Disconnecting, StateEvent::Disconnected) => Ok(Self::Disconnected),
            (Self::Connecting, StateEvent::Failure)
            | (Self::Connected, StateEvent::Failure)
            | (Self::Disconnecting, StateEvent::Failure) => Ok(Self::Error),
            _ => anyhow::bail!("Invalid event {event:?} in state {self:?}"),
        }
    }
}

pub fn default_config_path() -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("APPDATA is not set"))?;
    #[cfg(target_os = "macos")]
    let root =
        PathBuf::from(std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?)
            .join("Library/Application Support");
    #[cfg(all(unix, not(target_os = "macos")))]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| anyhow::anyhow!("No config directory"))?;
    Ok(root.join("RosesVPN").join("servers").join("wireguard.conf"))
}

pub fn profile_directory() -> anyhow::Result<PathBuf> {
    Ok(default_config_path()?.parent().unwrap().to_path_buf())
}

pub fn lockdown_config_path() -> anyhow::Result<PathBuf> {
    Ok(profile_directory()?
        .parent()
        .unwrap()
        .join("lockdown")
        .join("RosesVPN-Lockdown.conf"))
}

pub fn profiles() -> anyhow::Result<Vec<VpnProfile>> {
    let folder = profile_directory()?;
    let mut result = Vec::new();
    if !folder.exists() {
        return Ok(result);
    }
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("conf") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if validate_wireguard_config(&text).is_err() {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("WireGuard")
            .to_string();
        let endpoint = text
            .lines()
            .find_map(|line| {
                line.split_once('=')
                    .filter(|(k, _)| k.trim() == "Endpoint")
                    .map(|(_, v)| v.trim().to_string())
            })
            .unwrap_or_default();
        result.push(VpnProfile {
            name,
            endpoint,
            path,
        });
    }
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(result)
}

pub fn install_config(source: &Path) -> anyhow::Result<PathBuf> {
    let text = fs::read_to_string(source)?;
    validate_wireguard_config(&text)?;
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("wireguard");
    let safe: String = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .take(32)
        .collect();
    let target = profile_directory()?.join(format!(
        "{}.conf",
        if safe.is_empty() { "wireguard" } else { &safe }
    ));
    fs::create_dir_all(target.parent().unwrap())?;
    fs::copy(source, &target)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
    }
    Ok(target)
}

pub fn validate_wireguard_config(text: &str) -> anyhow::Result<()> {
    if Protocol::from_config(text)? != Protocol::WireGuard {
        anyhow::bail!("OpenVPN profiles are recognized but not supported yet")
    }
    for key in [
        "PrivateKey",
        "Address",
        "PublicKey",
        "Endpoint",
        "AllowedIPs",
    ] {
        if !text.lines().any(|line| {
            line.split_once('=')
                .is_some_and(|(k, v)| k.trim() == key && !v.trim().is_empty())
        }) {
            anyhow::bail!("WireGuard config is missing {key}");
        }
    }
    if text.lines().filter(|line| line.trim() == "[Peer]").count() != 1 {
        anyhow::bail!("This first native backend accepts one WireGuard peer per imported profile.");
    }
    if !text.lines().any(|line| {
        line.split_once('=').is_some_and(|(k, v)| {
            k.trim() == "AllowedIPs"
                && v.split(',')
                    .any(|ip| ip.trim() == "0.0.0.0/0" || ip.trim() == "::/0")
        })
    }) {
        anyhow::bail!("A full-tunnel config must include 0.0.0.0/0 and/or ::/0 in AllowedIPs.");
    }
    Ok(())
}

pub fn connect(config: &Path) -> anyhow::Result<String> {
    let text = fs::read_to_string(config)?;
    validate_wireguard_config(&text)?;
    #[cfg(target_os = "windows")]
    {
        let _ = native_stop_marker(config)?;
        run_elevated("install", &config.as_os_str().to_string_lossy())?;
        return Ok("WireGuardNT adapter configured".into());
    }
    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "linux")]
        {
            run_linux_privileged("install", config)?;
            return Ok(String::new());
        }
        #[cfg(not(target_os = "linux"))]
        anyhow::bail!("Native WireGuard interface management is not implemented on this platform.");
    }
}

pub fn disconnect() -> anyhow::Result<()> {
    disconnect_config(&default_config_path()?)
}

pub fn disconnect_config(config: &Path) -> anyhow::Result<()> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        signal_native_tunnel_stop(config)?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = config;
        anyhow::bail!("Native tunnel disconnect is not supported on this platform.")
    }
}

pub fn set_lockdown(
    enabled: bool,
    vpn_connected: bool,
    connected_profile: Option<&Path>,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let config = connected_profile.map(Path::to_path_buf).map_or_else(
            || {
                profiles()?
                    .into_iter()
                    .next()
                    .map(|p| p.path)
                    .map_or_else(default_config_path, Ok)
            },
            Ok,
        )?;
        run_linux_privileged_with_argument(
            if enabled {
                "lockdown-on"
            } else {
                "lockdown-off"
            },
            &format!(
                "{}|{}",
                config.display(),
                if vpn_connected {
                    "connected"
                } else {
                    "disconnected"
                }
            ),
        )?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        let argument = connected_profile.map(Path::to_path_buf).map_or_else(
            || {
                profiles()?
                    .into_iter()
                    .next()
                    .map(|p| p.path)
                    .map_or_else(default_config_path, Ok)
            },
            Ok,
        )?;
        let action = if enabled {
            "lockdown-on"
        } else {
            "lockdown-off"
        };
        let argument = format!(
            "{}|{}",
            argument.display(),
            if vpn_connected {
                "connected"
            } else {
                "disconnected"
            }
        );
        run_elevated(action, &argument)?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    anyhow::bail!("Lockdown Mode is not supported on this platform.")
}

pub fn lockdown_active() -> bool {
    #[cfg(target_os = "linux")]
    {
        return lockdown_config_path()
            .ok()
            .is_some_and(|path| path.exists())
            && Command::new("nft")
                .args(["list", "table", "inet", "rosesvpn"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
    }
    #[cfg(not(target_os = "linux"))]
    lockdown_config_path()
        .ok()
        .is_some_and(|path| path.exists())
}

#[cfg(target_os = "windows")]
fn run_elevated(action: &str, argument: &str) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError},
        UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
    };

    let response_path = std::env::temp_dir().join(format!(
        "rosesvpn-admin-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&response_path)?;
    let verb = "runas\0".encode_utf16().collect::<Vec<_>>();
    let executable = std::env::current_exe()?;
    let executable = executable
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let directory = std::env::current_exe()?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("RosesVPN executable has no parent directory"))?;
    let directory = directory
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let arguments = format!(
        "--admin-helper {action} \"{argument}\" \"{}\"",
        response_path.display()
    )
    .encode_utf16()
    .chain([0])
    .collect::<Vec<_>>();
    // Elevate RosesVPN itself so Windows identifies the app, not its tunnel implementation, in UAC.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = executable.as_ptr();
    info.lpParameters = arguments.as_ptr();
    info.lpDirectory = directory.as_ptr();
    info.nShow = 0;
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let _ = std::fs::remove_file(&response_path);
        anyhow::bail!(
            "Windows administrator approval was canceled or failed (Windows error {}).",
            unsafe { GetLastError() }
        );
    }
    if info.hProcess.is_null() {
        let _ = std::fs::remove_file(&response_path);
        anyhow::bail!("Windows did not return the elevated RosesVPN process handle.");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let message = std::fs::read_to_string(&response_path).unwrap_or_default();
        if message == "OK" {
            unsafe { CloseHandle(info.hProcess) };
            let _ = std::fs::remove_file(&response_path);
            return Ok(());
        }
        if !message.is_empty() {
            unsafe { CloseHandle(info.hProcess) };
            let _ = std::fs::remove_file(&response_path);
            anyhow::bail!("{message}");
        }
        if std::time::Instant::now() >= deadline {
            unsafe { CloseHandle(info.hProcess) };
            let _ = std::fs::remove_file(&response_path);
            anyhow::bail!("Timed out while starting the native WireGuardNT adapter.");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(target_os = "linux")]
fn run_linux_privileged(action: &str, config: &Path) -> anyhow::Result<()> {
    let _ = native_stop_marker(config)?;
    run_linux_privileged_with_argument(action, &config.to_string_lossy())
}

#[cfg(target_os = "linux")]
fn run_linux_privileged_with_argument(action: &str, argument: &str) -> anyhow::Result<()> {
    let response_path = std::env::temp_dir().join(format!(
        "rosesvpn-admin-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::write(&response_path, "")?;
    let mut child = Command::new("pkexec")
        .arg(std::env::current_exe()?)
        .arg("--admin-helper")
        .arg(action)
        .arg(argument)
        .arg(&response_path)
        .spawn()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let message = fs::read_to_string(&response_path).unwrap_or_default();
        if message == "OK" {
            let _ = fs::remove_file(&response_path);
            return Ok(());
        }
        if !message.is_empty() {
            let _ = fs::remove_file(&response_path);
            anyhow::bail!("{message}");
        }
        if let Some(status) = child.try_wait()? {
            let _ = fs::remove_file(&response_path);
            anyhow::bail!("Administrator helper ended before starting the tunnel ({status}).");
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = fs::remove_file(&response_path);
            anyhow::bail!("Timed out while starting the native WireGuard interface.");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Entry point used only by the authenticated administrator child process.
pub fn run_admin_helper(mut args: impl Iterator<Item = OsString>) -> anyhow::Result<()> {
    let action = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| anyhow::anyhow!("Missing administrator operation"))?;
    let argument = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing administrator operation argument"))?;
    let response_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Missing administrator response path"))?;
    if args.next().is_some() {
        anyhow::bail!("Unexpected administrator operation arguments");
    }
    let temp = std::env::temp_dir();
    if response_path.parent() != Some(temp.as_path())
        || !response_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rosesvpn-admin-") && name.ends_with(".txt"))
    {
        anyhow::bail!("Invalid administrator response path");
    }
    adopt_profile_environment(&action, &argument);
    #[cfg(target_os = "windows")]
    if let Some(parent) = std::env::current_exe()?.parent() {
        std::env::set_current_dir(parent)?;
    }
    match run_admin_tunnel_action(&action, &argument, &response_path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::write(response_path, format!("{error:#}"));
            Err(error)
        }
    }
}

/// UAC or pkexec may use a different administrator account and therefore different config paths.
fn adopt_profile_environment(action: &str, argument: &std::ffi::OsStr) {
    let raw = argument.to_string_lossy();
    let profile = if action == "install" {
        PathBuf::from(raw.as_ref())
    } else if action == "lockdown-on" || action == "lockdown-off" {
        PathBuf::from(raw.split('|').next().unwrap_or_default())
    } else {
        return;
    };
    if !is_managed_profile(&profile) {
        return;
    }
    let Some(config_root) = profile
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
    else {
        return;
    };
    #[cfg(target_os = "windows")]
    unsafe {
        std::env::set_var("APPDATA", config_root);
    }
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", config_root);
    }
}

fn native_interface_name(config: &Path) -> anyhow::Result<String> {
    let stem = config
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid tunnel name"))?;
    let mut name = String::from("rvpn-");
    for ch in stem.chars() {
        if name.len() >= if cfg!(target_os = "linux") { 14 } else { 64 } {
            break;
        }
        name.push(if ch.is_ascii_alphanumeric() || ch == '-' {
            ch
        } else {
            '-'
        });
    }
    Ok(name.trim_end_matches('-').to_string())
}

#[cfg(target_os = "windows")]
fn path_component_eq(component: &std::ffi::OsStr, expected: &str) -> bool {
    component.to_string_lossy().eq_ignore_ascii_case(expected)
}

#[cfg(not(target_os = "windows"))]
fn path_component_eq(component: &std::ffi::OsStr, expected: &str) -> bool {
    component == expected
}

#[cfg(target_os = "windows")]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(target_os = "windows"))]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

fn is_managed_profile(path: &Path) -> bool {
    let Some(server_dir) = path.parent() else {
        return false;
    };
    let app_dir = server_dir.parent();
    server_dir
        .file_name()
        .is_some_and(|name| path_component_eq(name, "servers"))
        && app_dir
            .and_then(Path::file_name)
            .is_some_and(|name| path_component_eq(name, "RosesVPN"))
        && path
            .extension()
            .is_some_and(|extension| path_component_eq(extension, "conf"))
}

fn native_stop_marker(config: &Path) -> anyhow::Result<PathBuf> {
    let name = native_interface_name(config)?;
    let folder = profile_directory()?.join("control");
    fs::create_dir_all(&folder)?;
    Ok(folder.join(format!("{name}.stop")))
}

fn native_stats_path(config: &Path) -> anyhow::Result<PathBuf> {
    let name = native_interface_name(config)?;
    let folder = profile_directory()?.join("control");
    fs::create_dir_all(&folder)?;
    Ok(folder.join(format!("{name}.stats")))
}

fn signal_native_tunnel_stop(config: &Path) -> anyhow::Result<()> {
    let marker = native_stop_marker(config)?;
    fs::write(&marker, "disconnect")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if !marker.exists() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    anyhow::bail!("The elevated RosesVPN tunnel process did not stop in time.")
}

fn run_admin_tunnel_action(
    action: &str,
    argument: &std::ffi::OsStr,
    response: &Path,
) -> anyhow::Result<()> {
    use defguard_wireguard_rs::{Kernel, WGApi, WireguardInterfaceApi};
    match action {
        "lockdown-on" | "lockdown-off" => {
            #[cfg(target_os = "linux")]
            {
                let value = argument.to_string_lossy();
                let mut parts = value.split('|');
                let profile = Path::new(parts.next().unwrap_or_default());
                let connected = parts.next() == Some("connected");
                linux_firewall::set_lockdown(action == "lockdown-on", connected, profile)?;
            }
            #[cfg(target_os = "windows")]
            windows_firewall::set_lockdown(action == "lockdown-on", argument)?;
            fs::write(response, "OK")?;
            Ok(())
        }
        "install" => {
            let path = PathBuf::from(argument).canonicalize()?;
            if !is_managed_profile(&path) {
                anyhow::bail!(
                    "RosesVPN refused a tunnel profile outside a RosesVPN\\servers directory"
                );
            }
            let expected_dir = profile_directory()?.canonicalize()?;
            if !path
                .parent()
                .is_some_and(|parent| paths_equal(parent, &expected_dir))
            {
                anyhow::bail!(
                    "RosesVPN refused a profile outside the selected user's managed profile folder"
                );
            }
            let text = fs::read_to_string(&path)?;
            validate_wireguard_config(&text)?;
            #[cfg(target_os = "linux")]
            linux_firewall::enable_kill_switch(&path)?;
            let (config, addresses) = parse_interface_config(&text, native_interface_name(&path)?)?;
            let mut api = WGApi::<Kernel>::new(config.name.clone())?;
            if let Err(error) = api.create_interface() {
                #[cfg(target_os = "windows")]
                if !lockdown_active() {
                    let _ = windows_firewall::remove();
                }
                #[cfg(target_os = "linux")]
                if !lockdown_active() {
                    let _ = linux_firewall::remove();
                }
                return Err(error.into());
            }
            if let Err(error) = api.configure_interface(&config).and_then(|()| {
                addresses
                    .iter()
                    .try_for_each(|address| api.assign_address(address))
            }) {
                let _ = api.remove_interface();
                #[cfg(target_os = "windows")]
                if !lockdown_active() {
                    let _ = windows_firewall::remove();
                }
                #[cfg(target_os = "linux")]
                if !lockdown_active() {
                    let _ = linux_firewall::remove();
                }
                return Err(error.into());
            }
            // IP Helper cannot resolve the adapter LUID until addresses are assigned.
            // Install WFP filters now, before adding VPN routes to avoid an unprotected gap.
            #[cfg(target_os = "windows")]
            if let Err(error) = windows_firewall::enable_kill_switch(&path) {
                let _ = api.remove_interface();
                if !lockdown_active() {
                    let _ = windows_firewall::remove();
                }
                return Err(error);
            }
            if let Err(error) = api.configure_peer_routing(&config.peers) {
                let _ = api.remove_interface();
                #[cfg(target_os = "windows")]
                if !lockdown_active() {
                    let _ = windows_firewall::remove();
                }
                #[cfg(target_os = "linux")]
                if !lockdown_active() {
                    let _ = linux_firewall::remove();
                }
                return Err(error.into());
            }
            let marker = native_stop_marker(&path)?;
            let stats_path = native_stats_path(&path)?;
            let _ = fs::remove_file(&marker);
            write_native_stats(&api, &stats_path)?;
            fs::write(response, "OK")?;
            while !marker.exists() {
                std::thread::sleep(std::time::Duration::from_secs(1));
                write_native_stats(&api, &stats_path)?;
            }
            api.remove_interface()?;
            #[cfg(target_os = "linux")]
            if !lockdown_active() {
                linux_firewall::remove()?;
            }
            #[cfg(target_os = "windows")]
            if !lockdown_active() {
                windows_firewall::remove()?;
            }
            let _ = fs::remove_file(stats_path);
            let _ = fs::remove_file(marker);
            Ok(())
        }
        _ => anyhow::bail!("Unknown administrator operation"),
    }
}

#[cfg(target_os = "linux")]
mod linux_firewall {
    use super::*;
    use std::{
        io::Write,
        net::ToSocketAddrs,
        process::{Command, Stdio},
    };

    fn nft(script: &str) -> anyhow::Result<()> {
        let mut child = Command::new("nft")
            .args(["-f", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Could not start nftables (install nftables): {e}"))?;
        child.stdin.take().unwrap().write_all(script.as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            anyhow::bail!(
                "nftables rejected the RosesVPN policy: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    fn rules(config: Option<&Path>) -> anyhow::Result<String> {
        let mut out = String::from(
            "add table inet rosesvpn\nadd chain inet rosesvpn output { type filter hook output priority -300; policy drop; }\nadd rule inet rosesvpn output oifname lo accept\nadd rule inet rosesvpn output oifname \"rvpn-*\" accept\nadd rule inet rosesvpn output ip protocol udp udp sport 68 udp dport 67 accept\nadd rule inet rosesvpn output ip6 nexthdr icmpv6 icmpv6 type { nd-neighbor-solicit, nd-neighbor-advert, nd-router-solicit, nd-router-advert } accept\n",
        );
        if let Some(path) = config.filter(|path| path.exists()) {
            let text = fs::read_to_string(path)?;
            let endpoint = text.lines().find_map(|line| {
                line.split_once('=')
                    .filter(|(k, _)| k.trim() == "Endpoint")
                    .map(|(_, v)| v.trim().to_owned())
            });
            if let Some(endpoint) = endpoint {
                let addresses = endpoint.to_socket_addrs().map_err(|e| {
                    anyhow::anyhow!("Cannot resolve VPN endpoint while installing Kill Switch: {e}")
                })?;
                let mut found = false;
                for addr in addresses {
                    let address = addr.ip();
                    let port = addr.port();
                    if address.is_ipv4() {
                        out.push_str(&format!("add rule inet rosesvpn output ip daddr {address} udp dport {port} accept\n"));
                    } else {
                        out.push_str(&format!("add rule inet rosesvpn output ip6 daddr {address} udp dport {port} accept\n"));
                    }
                    found = true;
                }
                if !found {
                    anyhow::bail!("VPN endpoint did not resolve to an address");
                }
            }
        }
        Ok(out)
    }

    fn replace(config: Option<&Path>) -> anyhow::Result<()> {
        let exists = Command::new("nft")
            .args(["list", "table", "inet", "rosesvpn"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        let mut script = String::new();
        if exists {
            script.push_str("delete table inet rosesvpn\n");
        }
        script.push_str(&rules(config)?);
        nft(&script)
    }

    pub(super) fn enable_kill_switch(config: &Path) -> anyhow::Result<()> {
        replace(Some(config))
    }
    pub(super) fn set_lockdown(
        enabled: bool,
        connected: bool,
        config: &Path,
    ) -> anyhow::Result<()> {
        if enabled {
            replace(connected.then_some(config))?;
            fs::create_dir_all(lockdown_config_path()?.parent().unwrap())?;
            fs::write(lockdown_config_path()?, b"active")?;
        } else {
            if connected {
                replace(Some(config))?;
            } else {
                remove()?;
            }
            let _ = fs::remove_file(lockdown_config_path()?);
        }
        Ok(())
    }
    pub(super) fn remove() -> anyhow::Result<()> {
        let exists = Command::new("nft")
            .args(["list", "table", "inet", "rosesvpn"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if exists {
            nft("delete table inet rosesvpn\n")?;
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod windows_firewall {
    use super::*;
    use std::net::ToSocketAddrs;
    use windows_sys::Win32::System::Rpc::RPC_C_AUTHN_WINNT;
    use windows_sys::{
        Win32::{
            Foundation::HANDLE, NetworkManagement::WindowsFilteringPlatform::*,
            Security::PSECURITY_DESCRIPTOR,
        },
        core::GUID,
    };
    const PROVIDER: GUID = GUID::from_u128(0x56dd38cc_9255_4f7a_a03c_ae7f29a0c9bf);
    const SUBLAYER: GUID = GUID::from_u128(0x603d0472_5156_4715_9df2_7a9d069915ac);
    const FILTER_V4: GUID = GUID::from_u128(0xd7267eaa_5e24_4d11_bcca_43ec482a15bc);
    const FILTER_V6: GUID = GUID::from_u128(0xaaa93920_0bd5_4ddd_8b19_ae4758ae7d49);
    const ENDPOINT_V4: GUID = GUID::from_u128(0x425aa894_e6c1_4353_81b7_2c15bc5509a8);
    const ENDPOINT_V6: GUID = GUID::from_u128(0x2d14a546_786e_4d22_a520_862eb2b835a9);

    fn check(code: u32, context: &str) -> anyhow::Result<()> {
        if code != 0 {
            anyhow::bail!("{context} failed (Windows error {code})");
        }
        Ok(())
    }
    fn name(s: &'static str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }
    fn open() -> anyhow::Result<HANDLE> {
        let mut engine: HANDLE = std::ptr::null_mut();
        check(
            unsafe {
                FwpmEngineOpen0(
                    std::ptr::null(),
                    RPC_C_AUTHN_WINNT,
                    std::ptr::null(),
                    std::ptr::null(),
                    &mut engine,
                )
            },
            "Opening Windows Filtering Platform",
        )?;
        Ok(engine)
    }
    fn delete(engine: HANDLE) {
        unsafe {
            FwpmFilterDeleteByKey0(engine, &FILTER_V4);
            FwpmFilterDeleteByKey0(engine, &FILTER_V6);
            FwpmSubLayerDeleteByKey0(engine, &SUBLAYER);
            FwpmProviderDeleteByKey0(engine, &PROVIDER);
            FwpmEngineClose0(engine);
        }
    }
    fn add_filter(
        engine: HANDLE,
        layer: GUID,
        key: GUID,
        label: &'static str,
        action: u32,
        condition: Option<FWPM_FILTER_CONDITION0>,
        weight: u64,
        persistent: bool,
    ) -> anyhow::Result<()> {
        let mut display_name = name(label);
        let mut description = name("Blocks outbound traffic while RosesVPN Lockdown is active");
        let display = FWPM_DISPLAY_DATA0 {
            name: display_name.as_mut_ptr(),
            description: description.as_mut_ptr(),
        };
        let mut filter = FWPM_FILTER0::default();
        filter.filterKey = key;
        filter.displayData = display;
        filter.flags = if persistent {
            FWPM_FILTER_FLAG_PERSISTENT
        } else {
            0
        };
        filter.providerKey = &PROVIDER as *const _ as *mut _;
        filter.layerKey = layer;
        filter.subLayerKey = SUBLAYER;
        filter.action = FWPM_ACTION0 {
            r#type: action,
            ..Default::default()
        };
        let mut condition = condition;
        if let Some(value) = condition.as_mut() {
            filter.numFilterConditions = 1;
            filter.filterCondition = value;
        }
        let mut weight_value = weight;
        if weight > 0 {
            filter.weight = FWP_VALUE0 {
                r#type: FWP_UINT64,
                Anonymous: FWP_VALUE0_0 {
                    uint64: &mut weight_value,
                },
            };
        }
        check(
            unsafe {
                FwpmFilterAdd0(
                    engine,
                    &filter,
                    std::ptr::null_mut() as PSECURITY_DESCRIPTOR,
                    std::ptr::null_mut(),
                )
            },
            "Adding Lockdown block filter",
        )
    }
    fn add_endpoint_filter(
        engine: HANDLE,
        layer: GUID,
        key: GUID,
        address: std::net::IpAddr,
        port: u16,
        persistent: bool,
    ) -> anyhow::Result<()> {
        let mut address_v6 = FWP_BYTE_ARRAY16 {
            byteArray16: [0; 16],
        };
        let (address_key, address_type, address_value) = match address {
            // WFP stores IPv4 addresses and ports in host byte order.
            std::net::IpAddr::V4(ip) => (
                FWPM_CONDITION_IP_REMOTE_ADDRESS,
                FWP_UINT32,
                FWP_CONDITION_VALUE0_0 {
                    uint32: u32::from(ip),
                },
            ),
            std::net::IpAddr::V6(ip) => {
                address_v6.byteArray16 = ip.octets();
                (
                    FWPM_CONDITION_IP_REMOTE_ADDRESS,
                    FWP_BYTE_ARRAY16_TYPE,
                    FWP_CONDITION_VALUE0_0 {
                        byteArray16: &mut address_v6,
                    },
                )
            }
        };
        let mut conditions = [
            FWPM_FILTER_CONDITION0 {
                fieldKey: address_key,
                matchType: FWP_MATCH_EQUAL,
                conditionValue: FWP_CONDITION_VALUE0 {
                    r#type: address_type,
                    Anonymous: address_value,
                },
            },
            FWPM_FILTER_CONDITION0 {
                fieldKey: FWPM_CONDITION_IP_REMOTE_PORT,
                matchType: FWP_MATCH_EQUAL,
                conditionValue: FWP_CONDITION_VALUE0 {
                    r#type: FWP_UINT16,
                    Anonymous: FWP_CONDITION_VALUE0_0 { uint16: port },
                },
            },
            FWPM_FILTER_CONDITION0 {
                fieldKey: FWPM_CONDITION_IP_PROTOCOL,
                matchType: FWP_MATCH_EQUAL,
                conditionValue: FWP_CONDITION_VALUE0 {
                    r#type: FWP_UINT8,
                    Anonymous: FWP_CONDITION_VALUE0_0 { uint8: 17 },
                },
            },
        ];
        let mut display_name = name("RosesVPN WireGuard server exception");
        let mut description =
            name("Allows WireGuard UDP only to the configured VPN server endpoint");
        let display = FWPM_DISPLAY_DATA0 {
            name: display_name.as_mut_ptr(),
            description: description.as_mut_ptr(),
        };
        let mut filter = FWPM_FILTER0::default();
        filter.filterKey = key;
        filter.displayData = display;
        filter.flags = if persistent {
            FWPM_FILTER_FLAG_PERSISTENT
        } else {
            0
        };
        filter.providerKey = &PROVIDER as *const _ as *mut _;
        filter.layerKey = layer;
        filter.subLayerKey = SUBLAYER;
        filter.action = FWPM_ACTION0 {
            r#type: FWP_ACTION_PERMIT,
            ..Default::default()
        };
        filter.numFilterConditions = conditions.len() as u32;
        filter.filterCondition = conditions.as_mut_ptr();
        let mut weight = u64::MAX;
        filter.weight = FWP_VALUE0 {
            r#type: FWP_UINT64,
            Anonymous: FWP_VALUE0_0 {
                uint64: &mut weight,
            },
        };
        check(
            unsafe {
                FwpmFilterAdd0(
                    engine,
                    &filter,
                    std::ptr::null_mut() as PSECURITY_DESCRIPTOR,
                    std::ptr::null_mut(),
                )
            },
            "Adding WireGuard server firewall exception",
        )
    }
    fn add_block(
        engine: HANDLE,
        layer: GUID,
        key: GUID,
        label: &'static str,
        persistent: bool,
    ) -> anyhow::Result<()> {
        add_filter(
            engine,
            layer,
            key,
            label,
            FWP_ACTION_BLOCK,
            None,
            0,
            persistent,
        )
    }
    fn set_policy(
        enabled: bool,
        connected_profile: Option<&Path>,
        persistent: bool,
    ) -> anyhow::Result<()> {
        let engine = open()?;
        unsafe {
            let _ = FwpmFilterDeleteByKey0(engine, &FILTER_V4);
            let _ = FwpmFilterDeleteByKey0(engine, &FILTER_V6);
            let _ = FwpmFilterDeleteByKey0(engine, &ENDPOINT_V4);
            let _ = FwpmFilterDeleteByKey0(engine, &ENDPOINT_V6);
            let _ = FwpmFilterDeleteByKey0(
                engine,
                &GUID::from_u128(0x9fc9b202_0a14_4e40_8610_6debd9188e34),
            );
            let _ = FwpmFilterDeleteByKey0(
                engine,
                &GUID::from_u128(0xd08ef387_a0c0_4f12_9640_c8a86e3e247d),
            );
            if enabled {
                let mut provider_name = name("RosesVPN");
                let mut provider_description = name("RosesVPN firewall policy");
                let provider = FWPM_PROVIDER0 {
                    providerKey: PROVIDER,
                    displayData: FWPM_DISPLAY_DATA0 {
                        name: provider_name.as_mut_ptr(),
                        description: provider_description.as_mut_ptr(),
                    },
                    flags: if persistent {
                        FWPM_PROVIDER_FLAG_PERSISTENT
                    } else {
                        0
                    },
                    ..Default::default()
                };
                let _ = FwpmProviderAdd0(
                    engine,
                    &provider,
                    std::ptr::null_mut() as PSECURITY_DESCRIPTOR,
                );
                let mut sublayer_name = name("RosesVPN");
                let mut sublayer_description = name("RosesVPN lockdown rules");
                let sublayer = FWPM_SUBLAYER0 {
                    subLayerKey: SUBLAYER,
                    displayData: FWPM_DISPLAY_DATA0 {
                        name: sublayer_name.as_mut_ptr(),
                        description: sublayer_description.as_mut_ptr(),
                    },
                    flags: if persistent {
                        FWPM_SUBLAYER_FLAG_PERSISTENT
                    } else {
                        0
                    },
                    providerKey: &PROVIDER as *const _ as *mut _,
                    weight: 0x7fff,
                    ..Default::default()
                };
                let _ = FwpmSubLayerAdd0(
                    engine,
                    &sublayer,
                    std::ptr::null_mut() as PSECURITY_DESCRIPTOR,
                );
            }
        }
        if enabled {
            if let Some(path) = connected_profile.filter(|path| path.exists()) {
                // Lockdown blocks all outbound traffic. Keep only the WireGuard
                // transport path to its configured endpoint open so handshake
                // packets can leave the physical adapter before the tunnel is up.
                let text = fs::read_to_string(path)?;
                let endpoint = text
                    .lines()
                    .find_map(|line| {
                        line.split_once('=')
                            .filter(|(key, _)| key.trim() == "Endpoint")
                            .map(|(_, value)| value.trim().to_owned())
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "WireGuard profile has no endpoint for the Lockdown exception"
                        )
                    })?;
                let endpoints = endpoint.to_socket_addrs()
                    .map_err(|error| anyhow::anyhow!("Resolving the WireGuard server for the Lockdown exception failed: {error}"))?
                    .collect::<Vec<_>>();
                let endpoint = endpoints.first().ok_or_else(|| {
                    anyhow::anyhow!("WireGuard server endpoint resolved to no addresses")
                })?;
                match endpoint.ip() {
                    std::net::IpAddr::V4(_) => add_endpoint_filter(
                        engine,
                        FWPM_LAYER_ALE_AUTH_CONNECT_V4,
                        ENDPOINT_V4,
                        endpoint.ip(),
                        endpoint.port(),
                        persistent,
                    )?,
                    std::net::IpAddr::V6(_) => add_endpoint_filter(
                        engine,
                        FWPM_LAYER_ALE_AUTH_CONNECT_V6,
                        ENDPOINT_V6,
                        endpoint.ip(),
                        endpoint.port(),
                        persistent,
                    )?,
                }
                let iface = native_interface_name(path)?;
                let dll = std::env::current_exe()?
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("RosesVPN executable has no parent directory"))?
                    .join("resources-windows/binaries/wireguard-amd64.dll");
                let wireguard = unsafe { wireguard_nt::load_from_path(dll) }
                    .map_err(|error| anyhow::anyhow!("Loading the bundled WireGuardNT API for the firewall exception failed: {error}"))?;
                let adapter = wireguard_nt::Adapter::open(&wireguard, &iface)
                    .map_err(|error| anyhow::anyhow!("Opening WireGuardNT adapter '{iface}' for the firewall exception failed: {error}"))?;
                let mut value = adapter.get_luid();
                if value == 0 {
                    anyhow::bail!("WireGuardNT returned an invalid LUID for adapter '{iface}'");
                }
                let condition = FWPM_FILTER_CONDITION0 {
                    fieldKey: FWPM_CONDITION_IP_LOCAL_INTERFACE,
                    matchType: FWP_MATCH_EQUAL,
                    conditionValue: FWP_CONDITION_VALUE0 {
                        r#type: FWP_UINT64,
                        Anonymous: FWP_CONDITION_VALUE0_0 { uint64: &mut value },
                    },
                };
                let permit_key4 = GUID::from_u128(0x9fc9b202_0a14_4e40_8610_6debd9188e34);
                let permit_key6 = GUID::from_u128(0xd08ef387_a0c0_4f12_9640_c8a86e3e247d);
                add_filter(
                    engine,
                    FWPM_LAYER_ALE_AUTH_CONNECT_V4,
                    permit_key4,
                    "RosesVPN tunnel IPv4",
                    FWP_ACTION_PERMIT,
                    Some(condition),
                    u64::MAX,
                    persistent,
                )?;
                add_filter(
                    engine,
                    FWPM_LAYER_ALE_AUTH_CONNECT_V6,
                    permit_key6,
                    "RosesVPN tunnel IPv6",
                    FWP_ACTION_PERMIT,
                    Some(condition),
                    u64::MAX,
                    persistent,
                )?;
            }
        }
        let result = if enabled {
            add_block(
                engine,
                FWPM_LAYER_ALE_AUTH_CONNECT_V4,
                FILTER_V4,
                "RosesVPN Lockdown IPv4",
                persistent,
            )
            .and_then(|_| {
                add_block(
                    engine,
                    FWPM_LAYER_ALE_AUTH_CONNECT_V6,
                    FILTER_V6,
                    "RosesVPN Lockdown IPv6",
                    persistent,
                )
            })
        } else {
            Ok(())
        };
        if enabled && result.is_err() {
            delete(engine);
        } else if !enabled {
            delete(engine);
        } else {
            unsafe {
                FwpmEngineClose0(engine);
            }
        }
        result
    }
    pub(super) fn set_lockdown(enabled: bool, argument: &std::ffi::OsStr) -> anyhow::Result<()> {
        let active = lockdown_config_path()?;
        let value = argument.to_string_lossy();
        let mut parts = value.split('|');
        let profile = PathBuf::from(parts.next().unwrap_or_default());
        let connected = parts.next() == Some("connected");
        if enabled {
            set_policy(true, connected.then_some(profile.as_path()), true)?;
            fs::create_dir_all(active.parent().unwrap())?;
            fs::write(active, b"active")?;
        } else {
            if connected {
                set_policy(true, Some(profile.as_path()), false)?;
            } else {
                set_policy(false, None, true)?;
            }
            let _ = fs::remove_file(active);
        }
        let _ = argument;
        Ok(())
    }
    pub(super) fn enable_kill_switch(config: &Path) -> anyhow::Result<()> {
        set_policy(true, Some(config), lockdown_active())
    }
    pub(super) fn remove() -> anyhow::Result<()> {
        set_policy(false, None, false)
    }
}

fn write_native_stats(
    api: &defguard_wireguard_rs::WGApi<defguard_wireguard_rs::Kernel>,
    path: &Path,
) -> anyhow::Result<()> {
    use defguard_wireguard_rs::WireguardInterfaceApi;
    let host = api.read_interface_data()?;
    let totals = host
        .peers
        .values()
        .fold(TransferStats::default(), |mut sum, peer| {
            sum.received_bytes = sum.received_bytes.saturating_add(peer.rx_bytes);
            sum.sent_bytes = sum.sent_bytes.saturating_add(peer.tx_bytes);
            sum
        });
    let updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    fs::write(
        path,
        format!("{updated} {} {}", totals.received_bytes, totals.sent_bytes),
    )?;
    Ok(())
}

fn parse_interface_config(
    text: &str,
    name: String,
) -> anyhow::Result<(
    defguard_wireguard_rs::InterfaceConfiguration,
    Vec<defguard_wireguard_rs::net::IpAddrMask>,
)> {
    use defguard_wireguard_rs::{InterfaceConfiguration, key::Key, net::IpAddrMask, peer::Peer};
    let mut section = "";
    let mut interface = std::collections::HashMap::new();
    let mut peer = std::collections::HashMap::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len() - 1];
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let target = if section.eq_ignore_ascii_case("Interface") {
            &mut interface
        } else if section.eq_ignore_ascii_case("Peer") {
            &mut peer
        } else {
            continue;
        };
        target.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    fn required<'a>(
        map: &'a std::collections::HashMap<String, String>,
        key: &str,
    ) -> anyhow::Result<&'a str> {
        map.get(key)
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("Config is missing {key}"))
    }
    let addresses = required(&interface, "address")?
        .split(',')
        .map(|value| IpAddrMask::from_str(value.trim()).map_err(Into::into))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut wg_peer = Peer::new(Key::from_str(required(&peer, "publickey")?)?);
    wg_peer.set_endpoint(required(&peer, "endpoint")?)?;
    wg_peer.set_allowed_ips(
        required(&peer, "allowedips")?
            .split(',')
            .map(|value| IpAddrMask::from_str(value.trim()).map_err(Into::into))
            .collect::<anyhow::Result<Vec<_>>>()?,
    );
    if let Some(key) = peer.get("presharedkey") {
        wg_peer.preshared_key = Some(Key::from_str(key)?);
    }
    if let Some(keepalive) = peer.get("persistentkeepalive") {
        wg_peer.persistent_keepalive_interval = keepalive.parse().ok();
    }
    let config = InterfaceConfiguration {
        name,
        prvkey: required(&interface, "privatekey")?.to_string(),
        addresses: addresses.clone(),
        port: interface
            .get("listenport")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        peers: vec![wg_peer],
        mtu: interface.get("mtu").and_then(|value| value.parse().ok()),
        fwmark: None,
    };
    Ok((config, addresses))
}

pub fn tunnel_status() -> String {
    tunnel_status_for(&default_config_path().unwrap_or_default())
}

pub fn tunnel_status_for(config: &Path) -> String {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        return if transfer_stats(config).is_ok() {
            "Connected".into()
        } else {
            "Disconnected".into()
        };
    }
    #[cfg(target_os = "macos")]
    {
        let interface = config.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        return match Command::new("wg").arg("show").arg("interfaces").output() {
            Ok(output)
                if output.status.success()
                    && String::from_utf8_lossy(&output.stdout)
                        .split_whitespace()
                        .any(|name| name == interface) =>
            {
                "Connected".into()
            }
            _ => "Disconnected".into(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const VALID: &str = "[Interface]\nPrivateKey = abc\nAddress = 10.0.0.2/32\n[Peer]\nPublicKey = xyz\nEndpoint = 192.0.2.1:51820\nAllowedIPs = 0.0.0.0/0, ::/0\n";
    #[test]
    fn recognizes_protocol_and_validates_full_tunnel() {
        assert_eq!(Protocol::from_config(VALID).unwrap(), Protocol::WireGuard);
        validate_wireguard_config(VALID).unwrap();
    }
    #[test]
    fn rejects_missing_secret_or_default_route() {
        assert!(validate_wireguard_config("[Interface]\nAddress=10.0.0.2/32\n").is_err());
        assert!(
            validate_wireguard_config(&VALID.replace("0.0.0.0/0, ::/0", "10.0.0.0/8")).is_err()
        );
    }
    #[test]
    fn rejects_multi_peer_profiles_until_multiple_peers_are_supported() {
        assert!(validate_wireguard_config(&format!("{VALID}[Peer]\nPublicKey = second\nEndpoint = 192.0.2.2:51820\nAllowedIPs = 10.1.0.0/16\n")).is_err());
    }
    #[test]
    fn native_config_parser_maps_interface_peer_and_full_tunnel_routes() {
        let profile = "[Interface]\nPrivateKey = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\nAddress = 10.0.0.2/32, fd00::2/128\nDNS = 10.0.0.1\n\n[Peer]\nPublicKey = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\nEndpoint = 192.0.2.1:51820\nAllowedIPs = 0.0.0.0/0, ::/0\nPersistentKeepalive = 25\n";
        let (parsed, addresses) = parse_interface_config(profile, "rvpn-test".into()).unwrap();
        assert_eq!(parsed.name, "rvpn-test");
        assert_eq!(parsed.addresses.len(), 2);
        assert_eq!(addresses.len(), 2);
        assert_eq!(parsed.peers.len(), 1);
        assert_eq!(parsed.peers[0].persistent_keepalive_interval, Some(25));
        assert_eq!(parsed.peers[0].allowed_ips.len(), 2);
    }
    #[test]
    fn follows_tunnel_lifecycle_and_fails_closed() {
        let s = TunnelState::Disconnected
            .transition(StateEvent::Connect)
            .unwrap();
        let s = s.transition(StateEvent::Failure).unwrap();
        assert_eq!(s, TunnelState::Error);
        assert_eq!(
            s.transition(StateEvent::Retry).unwrap(),
            TunnelState::Connecting
        );
        assert!(
            TunnelState::Disconnected
                .transition(StateEvent::Connected)
                .is_err()
        );
    }

    #[test]
    fn managed_profile_validation_does_not_depend_on_elevated_users_appdata() {
        let profile = PathBuf::from("user-profile")
            .join("AppData")
            .join("Roaming")
            .join("RosesVPN")
            .join("servers")
            .join("profile.conf");
        assert!(is_managed_profile(&profile));
        assert!(!is_managed_profile(&PathBuf::from(
            "downloads/profile.conf"
        )));
        assert!(!is_managed_profile(&profile.with_extension("txt")));
    }
}
