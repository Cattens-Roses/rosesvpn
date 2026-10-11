# RosesVPN

RosesVPN is an open-source desktop VPN client built with Rust and GPUI. The desktop app manages profiles and connection state; a separate Rust engine owns native tunnel setup and firewall policy.

> **Status:** WireGuard is the first supported protocol. This is an early-stage client, not a security-audited replacement for an established VPN provider's app. A VPN profile and a reachable VPN server are required to connect.

## Features

- Import and select WireGuard `.conf` profiles.
- Connect and disconnect from a graphical connection screen.
- Display connection state and transfer counters.
- Kill Switch blocks outbound traffic outside the tunnel while connecting and connected.
- Lockdown Mode keeps outbound traffic blocked after disconnect until the user disables it.
- Windows tunnel setup uses the bundled WireGuardNT API; Linux configures a kernel WireGuard interface through netlink. Neither platform starts `wireguard.exe` or `wg-quick`.
- The engine is a separate workspace crate so additional protocol backends can be added later. OpenVPN is not implemented yet.

Profiles must contain one peer and a full-tunnel route (`0.0.0.0/0` and/or `::/0`). Private profile files imported on Linux are stored with mode `0600`.

## Requirements

### Windows

- Windows 10 or later, x64.
- Rust 1.98.1 (the repository toolchain is pinned in `rust-toolchain.toml`).
- Administrator approval when starting the native tunnel or changing firewall policy.
- The signed WireGuardNT DLL is bundled under `resources-windows/binaries/`.

### Linux

- A Linux kernel with WireGuard support and the necessary `CAP_NET_ADMIN` privileges.
- `nftables`, `iproute2`, and `pkexec` plus a desktop authentication agent.
- Rust 1.98.1 and the GPUI build dependencies:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config cmake mold \
  libfontconfig1-dev libfreetype-dev libx11-dev libx11-xcb-dev \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxcb-xkb-dev libxcb-randr0-dev libxcb-xinerama0-dev libxcb-cursor-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev
```

Linux Lockdown currently uses runtime nftables rules. Re-enable Lockdown after reboot; boot-time firewall restoration is not implemented yet.

## Build and run

Build and launch the desktop client:

```sh
cargo build --locked -p rosesvpn
cargo run --locked -p rosesvpn
```

The app stores imported profiles in `%APPDATA%/RosesVPN/servers/` on Windows and `$XDG_CONFIG_HOME/RosesVPN/servers/` on Linux (or `$HOME/.config/RosesVPN/servers/` when `XDG_CONFIG_HOME` is unset). Import profiles through the app's profile controls. Connecting and toggling firewall protections asks the OS to authorize the privileged tunnel worker (`UAC` on Windows, `pkexec` on Linux).

## Install releases and update

Every successful push to `main` publishes a rolling [`continuous` release](https://github.com/Cattens-Roses/rosesvpn/releases/tag/continuous) and a versioned release with these assets:

- **Windows:** `RosesVPN-Setup.exe` installs per-user under `%LOCALAPPDATA%/Programs/RosesVPN`; the portable ZIP contains the app and WireGuardNT runtime.
- **Linux x64:** the `.deb` registers RosesVPN with the desktop and installs its runtime dependencies; the portable tarball can run from a writable folder.
- `update-manifest.json` and `CHECKSUMS.txt` identify the build and its SHA-256 hashes.

RosesVPN checks the continuous channel on launch and exposes **Settings → Software updates** for a manual check and install. It downloads the platform installer and verifies its size and SHA-256 before launching it. Windows updates open the per-user setup program. Debian installs ask the system authentication agent before applying the `.deb`; portable Linux installs should download the new tarball from Releases manually. Auto-update checks use the system `curl` HTTPS client.

## Development checks

```sh
cargo fmt --all --check
cargo check --locked -p rosesvpn
cargo test --locked -p vpn-engine
```

GitHub Actions runs format, check, and engine tests on Linux and Windows, and builds release binaries for both platforms. Linux CI checks compilation and behavior-level engine tests; it cannot validate a live tunnel without a VPN profile, a graphical authentication agent, and a host that permits network administration.

## Security model and limitations

The tunnel and firewall operations run in an elevated helper process. Windows uses WireGuardNT with Windows Filtering Platform; Linux uses the WireGuard kernel API and a dedicated nftables table. Kill Switch and Lockdown behavior is implemented in this project and has not received a production security audit. Review the policy and failure behavior before relying on it for sensitive traffic.

The design takes inspiration from [Mullvad's architecture](https://github.com/mullvad/mullvadvpn-app/blob/main/docs/architecture.md) and [security properties](https://github.com/mullvad/mullvadvpn-app/blob/main/docs/security.md); this project does not share Mullvad's production daemon, policy implementation, or audit status. Windows' native tunnel API is provided by [WireGuardNT](https://github.com/WireGuard/wireguard-nt).

## License

GPL-3.0-or-later. See [COPYING](./COPYING) and [THIRD-PARTY.md](./THIRD-PARTY.md).
