# AppImage builds and releases

Repository: https://github.com/AugusDogus/Compositor

## Running an AppImage

Download the `compositor-linux-x86_64` artifact from a successful [Actions run](https://github.com/AugusDogus/Compositor/actions/workflows/linux-release.yml) and extract it. GitHub requires sign-in to download Actions artifacts. Tagged builds are published to [Releases](https://github.com/AugusDogus/Compositor/releases).

```sh
chmod +x Compositor-*-x86_64.AppImage
./Compositor-0.1.0-x86_64.AppImage
```

Use the filename for the version you downloaded. If FUSE is unavailable, add `--appimage-extract-and-run`. Tools such as [Gear Lever](https://github.com/mijorus/gearlever) can add the AppImage and its icon to your desktop's application menu.

Requires x86_64 Linux with glibc 2.39 or newer, a Vulkan or OpenGL driver for the editor, and XDG portals for native file dialogs. Wayland and X11 are supported. The inference GPU path additionally needs Vulkan FP16 shader support. `COMPOSITOR_BACKGROUND_DEVICE=cpu` or `gpu` overrides automatic inference selection; `COMPOSITOR_INFERENCE_DIR` overrides the bundled model/runtime location.

## Build an AppImage locally

Start with the [source-build dependencies](linux-building.md), on Ubuntu 24.04 or the corresponding build container. Also install the packaging tools:

```sh
sudo apt-get install curl jq file unzip python3-venv desktop-file-utils
scripts/package-appimage.sh
scripts/prepare-release-assets.sh
scripts/check-appimage.sh dist/*.AppImage
```

The packaging scripts bundle native inference dependencies and prepare the models automatically. Python is used only during model preparation; no Python runtime is shipped. Releases are unsigned and include SHA-256 checksums for download integrity, not publisher authentication.

## Workflow

`.github/workflows/linux-release.yml` runs on pull requests, pushes to `main`, version tags, and manual dispatch. It installs Rust 1.94.0 on Ubuntu 24.04, checks formatting and Clippy, runs the ordinary tests with four test threads, builds the AppImage, and verifies the extracted payload. GPU and external-fixture tests remain explicit local checks.

Branch, pull-request and manual runs upload a `compositor-linux-x86_64` Actions artifact without publishing a release. Pushing `v<VERSION>` runs the same validation and then publishes a release. The tag must exactly match the package version in `Cargo.toml`. Prerelease versions produce prereleases. The release job alone receives `contents: write`; it does not use a personal access token or signing key.

1. Update `Cargo.toml` and refresh `Cargo.lock` with `cargo check`.
2. Commit and push the change to `main`; wait for the workflow to pass.
3. Create and push a version tag, for example `git tag v0.1.1` followed by `git push linux v0.1.1` (use your configured remote name).
4. Check the release workflow and download the resulting AppImage for a desktop smoke test.

Do not move published tags or overwrite released artifacts. Publish a new version for corrections. Re-running an already published tag fails instead of replacing existing release assets.

## Artifacts

- `Compositor-<VERSION>-x86_64.AppImage`: the desktop application with its icon, launcher, client libraries, image codecs, ONNX Runtime, the Vulkan/WebGPU plugin, both BiRefNet models and license notices.
- `Compositor-<VERSION>-linux-x86_64.bin`: a raw executable for existing standalone installations, which still require compatible system libraries.
- `.sha256` files: checksums for the two executable artifacts.
- `linux-update.json`: the version feed for standalone executable installations.

The AppImage requires glibc 2.39 or newer. It includes the libheif HEIC decoder plugin and dynamically loaded Wayland/X11 and graphics client libraries. It uses the host's graphics drivers and desktop portals. Native inference libraries and both full BiRefNet Dynamic models are bundled at build time. Background removal works offline without a setup command. The same artifact supports NVIDIA and AMD through Vulkan with FP16 shader support. Graphics drivers remain a host requirement; machines without a compatible hardware adapter select CPU inference automatically. Zstandard compression reduces the complete bundle's download size. Packaging enforces GitHub's 2 GiB per-asset limit.

## Local validation

The Wayland client library comes from the host alongside the graphics drivers. Bundling the older build-baseline client can prevent newer Mesa and NVIDIA EGL drivers from loading because required Wayland symbols are missing. The payload checker rejects bundles containing that library.

Run packaging on Ubuntu 24.04, or inside the corresponding build container, so dependencies are collected from the supported baseline rather than a newer host. `COMPOSITOR_LINUX_BINARY` selects an already compiled executable; `COMPOSITOR_APPIMAGE_TOOLS` selects the packaging-tool cache. Never bundle arbitrary libraries from the development host into a baseline release.

`scripts/check-appimage.sh` extracts the real artifact without FUSE and validates the launcher, icon, HEIC plugin and executable dependencies. Also launch the AppImage in an isolated desktop session and exercise image import and the Updates dialog before a release. Test a real removal using the extracted inference directory when native inference dependencies change. `COMPOSITOR_INFERENCE_TEST_BINARY` selects the compiled `linux_integrations` test executable and `COMPOSITOR_TEST_PHOTO` supplies a subject photo to the artifact checker. Run it on CPU in CI and Vulkan locally.

The workflow uses pinned GitHub Actions, linuxdeploy, appimagetool and an AppImage runtime. Packaging-tool downloads are verified against SHA-256 values from the publishers' release assets. ONNX model preparation uses a build-only Python environment, with operation equivalence tests and a pinned output checksum. No Python code or interpreter is shipped. Update these pins deliberately and revalidate the resulting AppImage.
