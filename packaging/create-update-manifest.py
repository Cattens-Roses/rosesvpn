#!/usr/bin/env python3
import hashlib
import json
import pathlib
import sys


def asset(path: pathlib.Path, base_url: str) -> dict[str, object]:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return {
        "name": path.name,
        "url": f"{base_url}{path.name}",
        "sha256": digest,
        "size": path.stat().st_size,
    }


def main() -> None:
    if len(sys.argv) != 4:
        raise SystemExit("Usage: create-update-manifest.py <dist-dir> <version> <commit>")
    dist = pathlib.Path(sys.argv[1])
    version = sys.argv[2]
    commit = sys.argv[3]
    base = "https://github.com/Cattens-Roses/rosesvpn/releases/download/continuous/"
    manifest = {
        "product": "RosesVPN",
        "version": version,
        "commit": commit,
        "platforms": {
            "windows-x64": {
                "installer": asset(dist / "RosesVPN-Setup.exe", base),
                "portable": asset(dist / "RosesVPN-Windows-Portable.zip", base),
            },
            "linux-x64": {
                "installer": asset(next(dist.glob("rosesvpn_*_amd64.deb")), base),
                "portable": asset(dist / "RosesVPN-Linux-Portable.tar.gz", base),
            },
        },
    }
    (dist / "update-manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
