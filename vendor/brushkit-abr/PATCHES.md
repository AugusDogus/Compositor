# Local patch to brushkit-abr 0.4.1

Source: <https://crates.io/crates/brushkit-abr/0.4.1>, MIT, Paweł Woźniak.

ZIP decoding in `src/parser.rs` reads at most the declared bitmap byte count plus one and rejects any size mismatch. Upstream reads up to 256 MiB and then truncates to the declared size, allowing a compressed small tip to allocate far more memory than its dimensions require.

Keep this patch until an upstream release provides equivalent bounded decoding. The importer regression `zip_expansion_is_bounded_by_declared_tip_dimensions` exercises rejection through the public deferred API.

The published crate omits its workspace-only `brushkit-fixture` development dependency. The local manifest restores version 0.4.1 so the retained upstream unit tests can run with `cargo test --manifest-path vendor/brushkit-abr/Cargo.toml --lib`.
