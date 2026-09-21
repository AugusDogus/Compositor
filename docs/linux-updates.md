# Linux updates

Linux releases are unsigned. No publisher signing key is required. SHA-256 assets provide download integrity checks, not publisher authentication.

## AppImage

The Updates dialog opens [GitHub Releases](https://github.com/AugusDogus/Compositor/releases/latest). Download the new AppImage, save your projects, close Compositor and replace the previous AppImage. The application does not try to overwrite its read-only mounted executable or trust an environment variable as a writable update destination. Automatic executable update checks are not used inside an AppImage.

## Standalone executable

Existing raw executable installations retain the HTTPS update feed and explicit download/install flow. The default feed is:

`https://github.com/AugusDogus/Compositor/releases/latest/download/linux-update.json`

`COMPOSITOR_UPDATE_URL` overrides it at build time. Each tagged release uploads its manifest alongside the referenced raw `.bin`, so the feed and executable belong to the same release. Stable versions do not offer prereleases or downgrades.

Requests have bounded sizes and deadlines. Cancellation leaves the current installation intact. Installation validates the architecture and ELF header, checks the staged file again, and atomically exchanges it with the running executable. It does not replace a different file based on `APPIMAGE`. Open projects remain in the running process; save and reopen after installation. The raw executable does not update system libraries.

## Preparing artifacts

Use `scripts/package-appimage.sh` and `scripts/prepare-release-assets.sh` on the Ubuntu 24.04 baseline. These scripts prepare files without publishing them. See [Linux releases](linux-releases.md) for the tag-triggered GitHub workflow.

The older `scripts/prepare-linux-update.sh` remains available for preparing a standalone binary and installation archive with the Podman baseline. It does not publish anything.
