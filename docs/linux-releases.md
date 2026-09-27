# Linux packages and releases

## Running an AppImage

[Download the latest release](https://github.com/AugusDogus/Compositor/releases/latest).

Make the downloaded file executable, then run it. Substitute its filename below:

```sh
chmod +x Compositor-0.6.1-x86_64.AppImage
./Compositor-0.6.1-x86_64.AppImage
```

If FUSE is unavailable, add `--appimage-extract-and-run`. [Gear Lever](https://github.com/mijorus/gearlever) can add the AppImage and icon to your application menu.

| Requirement | Supported configuration |
| --- | --- |
| Architecture | x86_64 |
| System libraries | glibc 2.39 or newer |
| Desktop | Wayland or X11, with XDG portals for file dialogs |
| Editor graphics | Working Vulkan or OpenGL driver |
| GPU inference | Compatible NVIDIA or AMD Vulkan GPU with FP16 shader support |
| CPU inference | Selected automatically when no compatible GPU is available |

The AppImage bundles image codecs, native inference libraries, BiRefNet and SAM 3.1. Background removal and object selection work offline without Python or dependency setup. Graphics drivers and desktop portals come from the host. `COMPOSITOR_BACKGROUND_DEVICE=cpu` or `gpu` overrides automatic inference selection.

Automatic update checks run on launch by default. Disable them in **Help > Check for Updates**. For AppImages, that dialog opens GitHub Releases: download the replacement, save your projects and close Compositor before replacing the old file. Releases are unsigned; SHA-256 files check download integrity, not publisher identity.

## Build an AppImage locally

Use Ubuntu 24.04 on x86_64 or ARM64 (aarch64) to retain the supported library baseline. Packaging targets the build host architecture. ARM64 builds are configured but have not yet been validated on ARM hardware; v0.6.1 remains x86_64-only. Install the [source-build dependencies](linux-building.md), then:

```sh
sudo apt-get install curl jq file unzip python3-venv desktop-file-utils
scripts/package-appimage.sh
scripts/prepare-release-assets.sh
scripts/check-appimage.sh dist/*.AppImage
```

Packaging prepares the models and native inference libraries automatically. Python is used only for model preparation. `COMPOSITOR_LINUX_BINARY` selects an already compiled executable; `COMPOSITOR_APPIMAGE_TOOLS` selects the packaging-tool cache. A prebuilt executable must also target the supported baseline.

## DEB, RPM and Arch packages

Development builds also produce DEB, RPM and Arch packages. They are not available in v0.6.1. All install the complete bundle, including offline models, under `/opt/compositor`, with a desktop launcher and `/usr/bin/compositor`. Host requirements match the AppImage; the package manager checks the declared system dependencies.

| Distribution | Install or update a downloaded package |
| --- | --- |
| Debian / Ubuntu | `sudo apt install ./compositor_<version>_amd64.deb` |
| Fedora | `sudo dnf install ./compositor-<version>-1.x86_64.rpm` |
| Arch Linux | `sudo pacman -U ./compositor-<version>-1-x86_64.pkg.tar.zst` |

Compositor's Updates dialog links to releases and does not replace package-managed files. These downloads do not add a package repository or an AUR entry.

To build all three packages from an existing, validated AppImage on Ubuntu 24.04:

```sh
sudo apt-get install dpkg-dev rpm zstd libarchive-tools
scripts/package-linux-native.sh dist/Compositor-0.6.1-x86_64.AppImage
```

Substitute the current version. DEB/RPM versions use `~` before prerelease identifiers; Arch versions use `pre.` (for example, `0.7.0pre.rc.1`). Each sorts before the corresponding stable version.

## Nix

The flake pins the published v0.6.1 AppImage, including its offline models, for x86_64 Linux.

From this checkout:

```sh
nix run .#compositor-bin
# Or install it in your profile:
nix profile install .#compositor-bin
```

On NixOS, enable graphics drivers and a desktop portal through your system configuration. Other distributions may need [nixGL](https://github.com/nix-community/nixGL) for hardware graphics. The package was built and launched with software rendering; hardware graphics through Nix remain unverified. Update this package through Nix.

## Publish a release

The [Linux workflow](../.github/workflows/linux-release.yml) runs on pull requests, pushes to `main`, version tags and manual dispatch. It checks formatting, Clippy and tests, builds the AppImage, and checks the payload with CPU inference and a real X-Trans RAW fixture. It then packages that payload as DEB, RPM and Arch files. The matrix builds on native x86_64 and ARM64 runners. Ordinary runs upload `compositor-linux-x86_64` and `compositor-linux-aarch64` Actions artifacts; version tags publish a GitHub release in this repository.

New pushes do not cancel running `main` or release-tag builds. Pull-request builds can be superseded by newer commits.

1. Update the package version in `Cargo.toml` and refresh `Cargo.lock` with `cargo check`.
2. Commit and push to `main`, then wait for validation to pass.
3. Create and push a `v<VERSION>` tag matching `Cargo.toml` exactly. Prerelease versions produce prereleases.
4. Check the release workflow and smoke-test the downloaded AppImage on a desktop.

Publish corrections under a new version. Existing release tags and artifacts are not overwritten.

| Release asset | Purpose |
| --- | --- |
| `Compositor-<VERSION>-<ARCH>.AppImage` | Complete desktop application with bundled models and licenses |
| `.deb`, `.rpm`, `.pkg.tar.zst` files | Complete bundle installed with a system package manager (development builds) |
| `Compositor-<VERSION>-linux-<ARCH>.bin` | Update binary for standalone installations; requires compatible system libraries |
| `.sha256` files | Checksums for executable and package assets |
| `linux-update.json` | Version feed used by the update dialog |

## Validation

`scripts/check-appimage.sh` extracts the artifact without FUSE and checks its launcher, icon, HEIC plugin and executable dependencies. The Wayland client library must come from the host: the checker rejects a bundled copy because older versions can prevent newer graphics drivers from loading.

For inference validation, `COMPOSITOR_INFERENCE_TEST_BINARY` selects the compiled `linux_integrations` test executable and `COMPOSITOR_TEST_PHOTO` supplies a subject photo. The workflow shows the complete CPU checks. Run the relevant [GPU checks](linux-port.md#gpu-rendering-and-brushes) and test inference on hardware when changing those paths. Also exercise image import, painting, save/reopen and updates in the packaged desktop application.

Packaging tools, model inputs and generated graphs are checksum-pinned. When updating them, revalidate the resulting artifact. GitHub's 2 GiB per-asset limit is enforced during packaging.

## Standalone installations

Contributors can create a `.tar.gz` installation archive with `scripts/package-linux-portable.sh` (requires Podman). `scripts/package-linux.sh` builds against the host libraries instead. These archives require the [system dependencies](linux-building.md#dependencies-and-launch) and separate inference setup.

Extract the archive and run `./install.sh` inside it. The default prefix is `~/.local`; pass another writable prefix as the first argument. The installer adds the executable, desktop launcher and icon. Ensure the prefix's `bin` directory is on `PATH`. Run the archive's `./setup-background.sh` to prepare inference libraries and models, following the build-time requirements in [source setup](linux-building.md#inference-in-source-builds).

Standalone installations use the [HTTPS version feed](https://github.com/AugusDogus/Compositor/releases/latest/download/linux-update.json) with explicit download and installation steps in the Updates dialog. Updates validate the binary and atomically replace the executable; they do not update system libraries. Save your projects and restart after installation. Stable versions do not offer prereleases or downgrades. `COMPOSITOR_UPDATE_URL` changes the feed at build time.

For development builds, `<ARCH>` is `x86_64` or `aarch64` (`amd64` or `arm64` in DEB filenames). Tagged releases require both architecture jobs to pass.
