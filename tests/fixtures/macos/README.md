# macOS project fixture

`mac-v10-text-effects.comp` was created on September 26, 2026 by upstream Swift model constructors and `ProjectStore.save` on a physical Intel Mac running macOS 15.8, using Swift 6.1.2 (Xcode 16.4).

Source: [Compositor `2309a85601824465aac5ebc1f45c4b8b9f78a5c1`](https://github.com/robbietilton/Compositor/tree/2309a85601824465aac5ebc1f45c4b8b9f78a5c1). The source `ProjectStore.swift` SHA-256 is `db3b2972e3a160277d0ab2665f371fa6250d0c64ae928fc3cd82e17bcde4eda1`.

The harness extracted upstream Codable types and validators, omitted UI/render-only members, filter dispatch and the unused imported raster cache, and removed type-level `nonisolated` syntax unsupported by Swift 6.1. Default isolation remained nonisolated. ProjectStore decoding, validation, PNG I/O, file coordination and package writing were retained.

The fixture includes Unicode text with UTF-16 color ranges, paragraph bounds, all six effects, a transformed layer, a guide and document resolution. Its cached image is a synthetic sRGB gradient, not rendered text. The test verifies metadata and cached pixel persistence, not font or effect rendering.

This fixture was generated independently of the Linux writer. It was not saved from the released macOS GUI, which requires macOS 26.5. See the [cross-device report](../../../docs/macos-compatibility.md).
