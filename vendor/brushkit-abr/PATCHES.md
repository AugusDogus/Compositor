# Local patch to brushkit-abr 0.4.1

Source: <https://crates.io/crates/brushkit-abr/0.4.1>, MIT, Paweł Woźniak.

ZIP decoding in `src/parser.rs` reads at most the declared bitmap byte count plus one and rejects any size mismatch. Upstream reads up to 256 MiB and then truncates to the declared size, allowing a compressed small tip to allocate far more memory than its dimensions require.

Keep this patch until an upstream release provides equivalent bounded decoding. The importer regression `zip_expansion_is_bounded_by_declared_tip_dimensions` exercises rejection through the public deferred API.

The published crate omits its workspace-only `brushkit-fixture` development dependency. The local manifest restores version 0.4.1 so the retained upstream unit tests can run with `cargo test --manifest-path vendor/brushkit-abr/Cargo.toml --lib`.

Record limits are enforced inside the parser before collecting samples, UUID recovery anchors, or preset descriptors. Each category has a 2048-record budget shared across blocks, and exceeding it fails the entire parse. Block and legacy-record collection use the same limit. Descriptor limit errors bypass the malformed-description fallback so a partial pack cannot hide the failure. Valid anchored sample frames do not scan their pixel bytes for recovery UUIDs.

The importer tests cover exact limits, one-block and cumulative recovery overflow, cumulative descriptor overflow, and UUID-shaped bytes inside valid pixels. These bounds prevent a single declared sample frame from hiding thousands of recovered records and allocating their metadata before the application can reject the pack. Pattern decoding remains unused: Compositor calls the deferred API that skips pattern blocks.

Dropped-tip diagnostics share each owner-name list with `Arc<[String]>`. Duplicate unused sample UUIDs previously copied the entire list, multiplying a long preset name by the number of dropped samples. Each preset has only one optional dual-brush UUID, so building the owner map copies each name at most once; all dropped records for that UUID then share it. Names and ordering are unchanged. The regression covers a 256 KiB preset name with 127 duplicate auxiliary samples and verifies shared storage, imported coverage, and spacing.
