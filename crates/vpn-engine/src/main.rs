use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut raw_args = std::env::args_os().skip(1);
    if raw_args.next().as_deref() == Some(std::ffi::OsStr::new("--admin-helper")) {
        return vpn_engine::run_admin_helper(raw_args);
    }
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("import") => {
            let source = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("Usage: rosesvpn import <wireguard.conf>"))?;
            println!(
                "Imported {}",
                vpn_engine::install_config(&PathBuf::from(source))?.display()
            );
        }
        Some("connect") => {
            let profile = vpn_engine::profiles()?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("Import a WireGuard profile first"))?;
            vpn_engine::connect(&profile.path)?;
            println!(
                "Connected using WireGuard. WireGuard for Windows enforces its built-in kill switch for full-tunnel configs."
            );
        }
        Some("disconnect") => {
            if let Some(profile) = vpn_engine::profiles()?.into_iter().next() {
                if vpn_engine::lockdown_active() {
                    vpn_engine::set_lockdown(true, false, Some(&profile.path))?;
                }
                vpn_engine::disconnect_config(&profile.path)?;
            }
            println!("Disconnected");
        }
        Some("status") => {
            let profile = vpn_engine::profiles()?.into_iter().next();
            if let Some(profile) = profile {
                println!("{}", vpn_engine::tunnel_status_for(&profile.path));
            } else {
                println!(
                    "{}",
                    if vpn_engine::lockdown_active() {
                        "Disconnected · Lockdown active"
                    } else {
                        "Disconnected"
                    }
                );
            }
        }
        Some("lockdown") => match args.next().as_deref() {
            Some("on") => {
                let connected = vpn_engine::profiles()?
                    .iter()
                    .any(|profile| vpn_engine::tunnel_status_for(&profile.path) == "Connected");
                let profile = vpn_engine::profiles()?.into_iter().next();
                vpn_engine::set_lockdown(
                    true,
                    connected,
                    profile.as_ref().map(|p| p.path.as_path()),
                )?;
                println!("Lockdown Mode enabled");
            }
            Some("off") => {
                vpn_engine::set_lockdown(false, false, None)?;
                println!("Lockdown Mode disabled");
            }
            Some("status") => println!(
                "{}",
                if vpn_engine::lockdown_active() {
                    "Lockdown Mode active"
                } else {
                    "Lockdown Mode inactive"
                }
            ),
            _ => anyhow::bail!("Usage: rosesvpn lockdown <on|off|status>"),
        },
        _ => {
            println!(
                "RosesVPN — WireGuard client\n\nCommands:\n  rosesvpn import <file.conf>  Import a WireGuard configuration\n  rosesvpn connect              Connect the first saved profile\n  rosesvpn disconnect           Disconnect\n  rosesvpn status               Show tunnel service status\n  rosesvpn lockdown <on|off|status>  Configure persistent Windows blocking\n\nOpenVPN is recognized for future support but not implemented yet."
            );
        }
    }
    Ok(())
}
