> [!IMPORTANT]
> This project is still very experimental, do not use it for anything serious.

# QuickGUI

QuickGUI is a native desktop GUI framework for **Go**, **TypeScript**, and **Rust**. Write windows and components in the language you already use; the same renderer, layout, and controls run on macOS (Windows and Linux compile, with native polish still in progress).

## Try it

Create a new project:

```console
bunx @quickgui/cli init my-app
```

You need Go 1.23+ (for Go apps), Bun 1.4+, a current Rust toolchain (for Rust apps), and Xcode Command Line Tools on macOS.

## Documentation

Guides and the component reference live in the [website](https://quickgui.dev).

Contributor notes (architecture, release, internals) are in [docs/](docs/README.md).

## License

MIT OR Apache-2.0.
