# Contributing to RosesVPN

RosesVPN is a desktop VPN client built with Rust and GPUI.

## Build

```sh
cargo run -p rosesvpn
cargo check -p rosesvpn
```

Keep VPN behavior in `crates/vpn-engine` and use the existing GPUI components and theme tokens
from `crates/ui`. Keep user-facing text concise.

## Licensing

The project is licensed under GPL-3.0-or-later. See [COPYING](./COPYING) and
[THIRD-PARTY.md](./THIRD-PARTY.md) for third-party licenses and attribution.
