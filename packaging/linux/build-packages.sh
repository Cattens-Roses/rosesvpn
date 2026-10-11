#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "Usage: $0 <release-binary> <version> <output-dir>" >&2
  exit 2
fi

binary=$(realpath "$1")
version=$2
output=$(realpath -m "$3")
root=$(mktemp -d)
portable="$root/RosesVPN-Linux-Portable"
debroot="$root/deb"
trap 'rm -rf "$root"' EXIT

mkdir -p "$output" "$portable" \
  "$debroot/DEBIAN" \
  "$debroot/usr/bin" \
  "$debroot/usr/share/applications" \
  "$debroot/usr/share/icons/hicolor/scalable/apps" \
  "$debroot/usr/share/doc/rosesvpn"

install -m 0755 "$binary" "$debroot/usr/bin/rosesvpn"
install -m 0644 assets/linux/rosesvpn.desktop "$debroot/usr/share/applications/rosesvpn.desktop"
install -m 0644 assets/linux/rosesvpn.svg "$debroot/usr/share/icons/hicolor/scalable/apps/rosesvpn.svg"
install -m 0644 COPYING "$debroot/usr/share/doc/rosesvpn/copyright"
install -m 0644 README.md "$debroot/usr/share/doc/rosesvpn/README.md"
install -m 0644 THIRD-PARTY.md "$debroot/usr/share/doc/rosesvpn/THIRD-PARTY.md"

cat > "$debroot/DEBIAN/control" <<EOF
Package: rosesvpn
Version: $version
Section: net
Priority: optional
Architecture: amd64
Maintainer: Cattens-Roses <https://github.com/Cattens-Roses/rosesvpn>
Depends: libfontconfig1, libfreetype6, libx11-6, libvulkan1, libwayland-client0, libxkbcommon0, nftables, iproute2, policykit-1, curl
Description: RosesVPN desktop WireGuard client
 Native Rust desktop VPN client with WireGuard, Kill Switch, and Lockdown Mode.
EOF

cat > "$debroot/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
exit 0
EOF
chmod 0755 "$debroot/DEBIAN/postinst"

dpkg-deb --build --root-owner-group "$debroot" "$output/rosesvpn_${version}_amd64.deb"

install -m 0755 "$binary" "$portable/rosesvpn"
install -m 0644 README.md "$portable/README.md"
install -m 0644 COPYING "$portable/LICENSE"
install -m 0644 THIRD-PARTY.md "$portable/THIRD-PARTY.md"
install -m 0644 assets/linux/rosesvpn.desktop "$portable/rosesvpn.desktop"
install -m 0644 assets/linux/rosesvpn.svg "$portable/rosesvpn.svg"
tar -C "$root" -czf "$output/RosesVPN-Linux-Portable.tar.gz" RosesVPN-Linux-Portable

echo "Created packages:"
ls -lh "$output/rosesvpn_${version}_amd64.deb" "$output/RosesVPN-Linux-Portable.tar.gz"
