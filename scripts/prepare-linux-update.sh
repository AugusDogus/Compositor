#!/usr/bin/env bash
# Prepare unsigned local artifacts. This script never publishes them.
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"
scripts/package-linux-portable.sh
package_version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')"
architecture="$(uname -m)"
artifact="dist/Compositor-$package_version-linux-$architecture.bin"
install -m755 target/linux-ubuntu24.04/release/compositor "$artifact"
python3 - "$package_version" "$architecture" "$artifact" <<'PY'
import datetime, json, pathlib, sys, urllib.parse
version, architecture, artifact = sys.argv[1:]
filename = pathlib.Path(artifact).name
manifest = {
    "version": version,
    "pub_date": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "notes": f"Compositor {version} for Linux.",
    "platforms": {
        f"linux-{architecture}": {
            "url": f"https://github.com/AugusDogus/Compositor/releases/download/v{urllib.parse.quote(version, safe='')}/{urllib.parse.quote(filename, safe='')}",
        }
    },
}
pathlib.Path("dist/linux-update.json").write_text(json.dumps(manifest, indent=2) + "\n")
PY
printf 'Prepared %s and dist/linux-update.json. Nothing has been uploaded.\n' "$artifact"
