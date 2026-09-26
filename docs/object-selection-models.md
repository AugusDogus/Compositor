# Object selection model

Compositor uses the **[SAM 3.1 interactive tracker](https://github.com/facebookresearch/sam3/blob/main/RELEASE_SAM3p1.md)** for point- and box-based object selection. BiRefNet handles Select Subject and background removal separately. Both run locally through native ONNX Runtime, with cached sessions and image features.

The AppImage bundles the models. Source builds prepare them using [the inference setup](linux-building.md#inference-in-source-builds). SAM 3.1 weights use Meta's SAM License; source revisions, graph checksums and license files are pinned in [object-selection-model.json](../scripts/object-selection-model.json).

## Quality and performance

A September 21, 2026 diagnostic compared 13 objects from five DAVIS 2017 first frames: `bike-packing`, `dogs-jump`, `horsejump-high`, `pigs` and `soapbox`. These were deliberately selected difficult examples, not a representative benchmark. Clicks used the deepest interior foreground pixel; boxes used the annotation bounds.

| Model | Click mean IoU | Box mean IoU | Median new-image click | Median cached click |
| --- | ---: | ---: | ---: | ---: |
| SAM 2.1 Small | 77.61% | Not measured | 885 ms | 78 ms |
| SAM 2.1 Large | 75.05% | 87.78% | 1,284 ms | 77 ms |
| SAM 3 tracker | 74.35% | 90.12% | 1,886 ms | 80 ms |
| SAM 3.1 tracker | 89.31% | 89.89% | 1,437 ms | 78 ms |
| UnSAMv2+, fixed granularity 1.0 | 59.42% | 78.39% | 730 ms | 96 ms |

Runs used FP16 ONNX inference on an NVIDIA RTX 3070 Ti through ONNX Runtime 1.30 and its WebGPU/Vulkan provider. New-image times include encoding and may include first-run setup; all times exclude model-session loading and UI presentation. Results and latency will vary with images, hardware and prompts.

SAM 3.1 had the strongest click results in this sample; SAM 3 was slightly better on boxes. This supports the current selection, not a claim of global state-of-the-art quality. Clicking a shirt or hat can select that part instead of the whole person. Accurate boxes can reduce ambiguity, and boundaries may still need manual correction.

Published results have different protocols. For example, the [SAM 3 paper's SA-37 comparison](https://arxiv.org/html/2511.16719v2#S6.T6.fig2) shows SAM 3 improving over SAM 2.1 Large after corrective clicks, while SAM 2.1 Large has a slightly better one-click average. SAM 3.1's release emphasizes video tracking and does not provide a separate still-image click comparison. The diagnostic above should not be substituted for either benchmark.

## Build and test

The committed [exporter](../scripts/object_selection_model.py) and [build requirements](../scripts/requirements-object-model-build.txt) produce the pinned native graphs. Python is needed at build time only. After preparing the native runtime with `scripts/setup-background.sh`, verify the installed object model and exercise the packaged selection path:

```sh
scripts/setup-object-selection.sh --check
COMPOSITOR_BACKGROUND_DEVICE=cpu \
cargo test --locked --lib packaged_object_selection -- --ignored --nocapture
COMPOSITOR_BACKGROUND_DEVICE=gpu \
cargo test --locked --lib packaged_object_selection -- --ignored --nocapture
```

The GPU test requires a compatible Vulkan adapter. Set `COMPOSITOR_INFERENCE_DIR` for a nondefault installation or an extracted AppImage's `usr/share/compositor/inference` directory. Running `scripts/setup-object-selection.sh` without `--check` rebuilds or installs missing model files.

The [native benchmark tests](../src/object_selection/inference/tests.rs) support your own subject photos and labeled fixtures. To measure point selection, substitute a photo and foreground coordinates:

```sh
COMPOSITOR_SEGMENTATION_MODEL="${XDG_DATA_HOME:-$HOME/.local/share}/compositor/inference/object-selection" \
COMPOSITOR_SEGMENTATION_ARCH=sam3 \
COMPOSITOR_SEGMENTATION_PHOTO=/path/to/photo.png \
COMPOSITOR_SEGMENTATION_POINTS='120,80;125,85' \
COMPOSITOR_SEGMENTATION_OUTPUT=target/object-selection-masks \
COMPOSITOR_BACKGROUND_DEVICE=gpu \
cargo test --locked --lib benchmark_native_prompt_model -- --ignored --nocapture
```

If you set `COMPOSITOR_INFERENCE_DIR`, use its `object-selection` subdirectory as `COMPOSITOR_SEGMENTATION_MODEL` too. The first point encodes the image; subsequent points reuse cached features. The test writes predicted masks and reports timings. This is a diagnostic tool, not an accuracy benchmark without independently labeled masks.
