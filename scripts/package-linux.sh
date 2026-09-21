#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"
package_binary="${COMPOSITOR_LINUX_BINARY:-target/release/compositor}"
if [[ -z "${COMPOSITOR_LINUX_BINARY:-}" ]]; then
    cargo build --locked --release
fi
[[ -f "$package_binary" && -x "$package_binary" ]] || { printf 'No executable found at %s. Build the release first.\n' "$package_binary" >&2; exit 1; }
package_version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')"
package_name="Compositor-$package_version-linux-$(uname -m)"
stage_dir="$(mktemp -d)"
trap 'rm -rf -- "$stage_dir"' EXIT
bundle_dir="$stage_dir/$package_name"
install -Dm755 "$package_binary" "$bundle_dir/bin/compositor"
install -Dm755 packaging/linux/install.sh "$bundle_dir/install.sh"
for script in setup-background.sh setup-object-selection.sh; do
    install -Dm755 "scripts/$script" "$bundle_dir/$script"
done
for file in background_model.py test_background_model.py requirements-inference-build.txt object-selection-model.json object_selection_model.py requirements-object-model-build.txt; do
    install -Dm644 "scripts/$file" "$bundle_dir/$file"
done
install -Dm644 packaging/linux/compositor.desktop "$bundle_dir/share/applications/compositor.desktop"
install -Dm644 Compositor/Assets.xcassets/AppIcon.appiconset/app-icon-256.png "$bundle_dir/share/icons/hicolor/256x256/apps/compositor.png"
install -Dm644 README.md "$bundle_dir/README.md"
install -Dm644 docs/linux-port.md "$bundle_dir/docs/linux-port.md"
install -Dm644 docs/linux-updates.md "$bundle_dir/docs/linux-updates.md"
for reference in README.md macos-typography.md macos-upstream-workspace.png macos-upstream-new-canvas.png macos-upstream-file-menu.png; do
    install -Dm644 "docs/references/$reference" "$bundle_dir/docs/references/$reference"
done
install -Dm644 LICENSE "$bundle_dir/LICENSE"
install -Dm644 vendor/quickgui/LICENSE-MIT "$bundle_dir/licenses/quickgui/LICENSE-MIT"
install -Dm644 vendor/quickgui/LICENSE-APACHE "$bundle_dir/licenses/quickgui/LICENSE-APACHE"
install -Dm644 vendor/quickgui/THIRD_PARTY_NOTICES.md "$bundle_dir/licenses/quickgui/THIRD_PARTY_NOTICES.md"
install -Dm644 assets/icons/LICENSE "$bundle_dir/licenses/lucide/LICENSE"
install -Dm644 assets/fonts/INTER-LICENSE.txt "$bundle_dir/licenses/inter/LICENSE.txt"
for notice in licenses/*; do
    install -Dm644 "$notice" "$bundle_dir/licenses/$(basename "$notice")"
done
mkdir -p dist
tar -C "$stage_dir" -czf "dist/$package_name.tar.gz" "$package_name"
printf 'Created %s/dist/%s.tar.gz\n' "$project_root" "$package_name"
