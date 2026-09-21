# Object selection model research

Research checked on 2026-09-21. **The Linux fork selects the SAM 3.1 interactive tracker.** This document separates released models, paper results, and community observations. A recent Hugging Face upload or an ONNX conversion does not establish a new model generation or better masks.

## Released models and newer candidates

| Model | Original release or paper date | Availability and relevant evidence |
|---|---|---|
| [SAM 2.1](https://github.com/facebookresearch/sam2) | September 2024 checkpoints | Released weights and point/box/mask prompting. Useful baseline, especially against the Large checkpoint rather than only Tiny. |
| [SAM 3](https://github.com/facebookresearch/sam3) | November 20, 2025 | Released weights. Adds text/concept segmentation and supports interactive instance segmentation. Its paper directly compares still-image clicks against SAM 2.1 Large. |
| [SAM 3.1](https://github.com/facebookresearch/sam3/blob/main/RELEASE_SAM3p1.md) | March 27, 2026 | Released weights. Object Multiplex improves joint multi-object video tracking. The release publishes video benchmarks, not a separate still-image click benchmark demonstrating superiority over SAM 3. |
| [EfficientSAM3](https://github.com/SimonZeng7108/efficientsam3) | Encoder releases December 2025 and January 2026; full fine-tuned models June 11, 2026 | Actual [HF weights](https://huggingface.co/Simon7108528/EfficientSAM3/tree/main/efficientsam3_ft) exist. EV-M/RV-M/TV-M contain 89.2M/92.7M/95.3M parameters for concept segmentation. Comparable current click-quality results are missing from its main README. |
| [SAM3-LiteText](https://arxiv.org/abs/2602.12173) | Paper February 12, 2026; checkpoint release February 18 | Reduces the text encoder while retaining SAM 3's vision encoder. Relevant to text prompting; it does not establish an improvement to point-based object boundaries. |
| [SegNext](https://github.com/uncbiag/SegNext) | CVPR 2024 | Released models and interactive evaluation code. Supports diverse prompts and detailed refinement. The similarly named SegNeXt semantic segmentation model is a different project. |
| [FocalClick-XL](https://arxiv.org/abs/2506.14686) | Paper June 17, 2025 | Promising click, scribble, box, coarse-mask, and alpha-matte results. No official XL weights or implementation were located. The upstream [release request](https://github.com/XavierCHEN34/ClickSEG/issues/20) has no reply. Existing FocalClick weights are not XL. |
| [SAM2Refiner](https://arxiv.org/abs/2502.09660) | Paper February 12, 2025 | Strong reported fine-detail results on HQSeg-style image datasets. No official released implementation or checkpoint was located in GitHub/HF searches. |
| [MobileSAM2](https://arxiv.org/abs/2607.12297) | Paper July 14, 2026 | A genuine newer lightweight SAM2 paper, with 5.8M/10.4M/23.7M variants. Published comparisons emphasize video segmentation and embodied tasks. No official repository, HF checkpoint, or directly comparable still-image click table was located. |
| [UnSAMv2 / UnSAMv2+](https://github.com/yujunwei04/UnSAMv2) | Official release November 17, 2025 | Apache 2.0 source and both checkpoints released. Adds granularity-conditioned point, box, and mask prompting to SAM2.1 Small. HF upload September 15, 2026 is not its release date. |
| [U-CFR](https://github.com/elidandar/UCFR-Interactive-Segmentation) | Preprint July 22, 2026; ICPR 2026 | Released code and a 377 MB checkpoint. Adds uncertainty-guided internal corrective clicks to a SimpleClick-derived model. Its improvements are benchmark-specific, not evidence of general superiority to SAM 3. |

No official Meta **SAM 4** release, paper, or Hugging Face model was found. Search results such as SAM4MIS and SAM4MLLM refer to applications of SAM, not a fourth-generation Meta model. No newer official SegNext successor was located.

Dates above refer to the original research or checkpoint release. For example, the EfficientSAM3 HF repository was updated July 1, 2026, but that upload timestamp is not a new architecture release. SAM 3.1's HF repository was created March 26 and released March 27.

## Direct still-image evidence

The [SAM 3 paper, Table 6](https://arxiv.org/html/2511.16719v2#S6.T6.fig2), reports the following on **SA-37 interactive image segmentation**. These are the authors' measurements, not this application's native latency.

| Model | 1-click mIoU | 3-click mIoU | 5-click mIoU | Reported FPS |
|---|---:|---:|---:|---:|
| SAM 1 H | 58.5 | 77.0 | 82.1 | 41.0 |
| SAM 2.1 L | 66.4 | 80.3 | 84.3 | 93.0 |
| SAM 3 | 66.1 | 81.3 | 85.1 | 43.5 |

SAM 3 improves the average after corrective clicks in this evaluation. SAM 2.1 Large has a slightly higher one-click average and higher reported throughput. This table does not evaluate box prompts, text prompts, SAM 3.1, or native Vulkan execution.

Other relevant evidence uses different data and prompting protocols:

- **SegNext:** its released fine-tuned ViT-B reports HQSeg-44K 5-click mIoU **91.75**, NoC90 **5.32**; DAVIS 5-click mIoU **91.87**, NoC90 **4.43**. These are useful detailed-mask baselines, not directly comparable to SA-37 averages.
- **FocalClick-XL:** its paper reports XL-B/XL-H DAVIS NoC90 **4.43/4.39**, versus SAM-H **5.34** in its evaluation. XL-B reports **220 ms online interaction**. The paper's comparison does not establish an advantage over SAM 2.1 Large or SAM 3, and public XL weights remain unverified.
- **SAM2Refiner:** its paper reports a **93.1 mIoU / 88.9 boundary mIoU** average with ten points across DIS, COIFT, HRSOD, and ThinObject, versus SAM2-L **78.5 / 71.5**. This is a fine-detail, multi-click evaluation, not evidence of one-click performance on general photographs. Released weights remain unverified.
- **U-CFR:** its paper reports Berkeley NoC90 **2.19** versus SimpleClick-B **2.46**, but DAVIS **5.54** versus **5.48**. Lower NoC is better. Its table does not compare against SAM 2.1 or SAM 3.

The [UnSAMv2 paper](https://arxiv.org/abs/2511.13714) reports average 1-click IoU of **69.0 / 79.3 / 81.7** for SAM2 / UnSAMv2 / UnSAMv2+ across GrabCut, Berkeley, DAVIS, SA-1B, and PartImageNet. Its interactive comparison selects optimal granularity from 0.1 to 1.0 in 0.1 steps. This oracle choice requires knowing the desired mask and is not equivalent to an ordinary fixed-setting click UI. Its HF card describes both checkpoints as unsupervised, while paper Table 5 marks UnSAMv2+ as using supervised and unsupervised training; the training claim should not be repeated without that qualification.

## EfficientSAM3 evidence and deployment caveats

EfficientSAM3 is a candidate for measurement, not an established quality winner:

- Its [arXiv manuscript](https://arxiv.org/html/2511.15833) still labels section 4.6 **“Planned Evaluation Protocol (No Results yet)”**. The current repository has newer releases than that manuscript, but its main model table reports parameter counts rather than comparable point/box accuracy.
- In [issue #13](https://github.com/SimonZeng7108/efficientsam3/issues/13), the maintainer reported on January 4, 2026 that early distilled image encoders exceeded **60% COCO mIoU** with geometry-only prompts. That statement does not establish the accuracy of June's full models against SAM 3 or SAM 2.1 Large under the same protocol.
- The [model builder](https://github.com/SimonZeng7108/efficientsam3/blob/main/sam3/sam3/model_builder.py) defaults to `enable_inst_interactivity=False`. Enabling it adds a SAM 3 interactive tracker. The advertised compact concept-model size should not be assumed to describe a complete validated click-based instance pipeline.
- Merged [ONNX export PR #49](https://github.com/SimonZeng7108/efficientsam3/pull/49) demonstrates open-vocabulary text inference and specialized exports. It does not demonstrate the interactive point tracker. The contributor also identified FP16 overflow in EfficientViT attention and retained FP32 for affected operations. Exportability and precision must be verified for the exact prompt path and backend.

## Community evidence

[ComfyUI's SAM 3/3.1 integration](https://github.com/Comfy-Org/ComfyUI/pull/13408), merged April 23, 2026, supports image boxes, points, text prompts, and mask refinement. Its author preferred the newer 3.1 weights in their testing. This demonstrates practical adoption, but supplies no controlled still-image quality comparison.

Hugging Face exposes official SAM 3.1 weights and active community conversions. Official weights require accepting Meta's SAM license; a community mirror does not change that license. Download counts reflect adoption, not segmentation quality.

Reddit search was attempted but returned HTTP 403 in this environment. No Reddit consensus is claimed. GitHub discussions, public HF metadata, official release notes, and papers are the evidence available here.

## Native diagnostic comparison

SAM 3.1 gave the strongest click results among the tested candidates, while SAM 3 was slightly ahead on boxes. We select SAM 3.1 for the Linux fork. These results cover **13 objects from five deliberately selected DAVIS 2017 first frames**, not a representative benchmark: `bike-packing`, `dogs-jump`, `horsejump-high`, `pigs`, and `soapbox`. Every annotated instance is included. Clicks use the deepest interior foreground pixel; boxes use the annotation bounds. Box results therefore assume accurately drawn boxes. Masks use the same signed-logit interpolation and foreground threshold. All runs use native FP16 ONNX inference on an NVIDIA RTX 3070 Ti through ONNX Runtime 1.30 and its WebGPU/Vulkan provider.

| Model / configuration | Click mean IoU | Box mean IoU | Median new-image click | Median cached click |
|---|---:|---:|---:|---:|
| SAM 2.1 Small | 77.61% | Not measured | 885 ms | 78 ms |
| SAM 2.1 Large | 75.05% | 87.78% | 1,284 ms | 77 ms |
| SAM 3 tracker | 74.35% | 90.12% | 1,886 ms | 80 ms |
| SAM 3.1 tracker | 89.31% | 89.89% | 1,437 ms | 78 ms |
| UnSAMv2+, fixed granularity 0.5 | 50.53% | Not measured | 734 ms | 96 ms |
| UnSAMv2+, fixed granularity 1.0 | 59.42% | 78.39% | 730 ms | 96 ms |
| UnSAMv2+, fixed 1.0 plus mask refinement | 61.48% | Not measured | 745 ms | 128 ms |

New-image times include image encoding and may include first-run setup; cached times reuse image features. They exclude model-session loading. Large and SAM 3 improve several difficult objects and box selection, despite lower click averages on this small sample. SAM 3.1 resolves the shirt/whole-person and hat/whole-cyclist failures in the displayed examples, but still misses some boundaries. This small diagnostic does not establish global state-of-the-art performance. Clicking a person's shirt or hat can select that part instead of the entire person. Accurate boxes reduce this ambiguity.

UnSAMv2+ exposes genuine part/whole control. Its training assigns 0.1 to the smallest nested mask and 1.0 to the largest. Fixed 1.0 recovers full people and pigs but can merge a rider with the horse or nearby soapbox occupants. Fixed 0.5 frequently selects body parts. Its second decoder pass uses the previous mask as input, following the official mask-to-mask refinement example. No setting was selected separately using each object's ground truth. These results do not support replacing the click model with UnSAMv2+ solely on its published average.

### Export and backend checks

SAM 3.1 uses the pinned 3.1 checkpoint's shared image backbone and interactive prompt/mask decoder. The original eager FP32 implementation and the mapped export produced identical binary masks on the validation image. Decoder maximum absolute differences were `1.34e-5` for mask logits and `2.03e-6` for scores. FP32 ONNX preserved mask IoU 1.0; FP16 mask IoU against the reference ranged from 99.85% to 99.96% across the tested point, box, and refinement prompts. The exporter retains the original raw IoU scores and validates dynamic point/box inputs with native CPU ONNX Runtime. Source-build setup verifies pinned source and output hashes before installation; the AppImage bundles the native graphs and license without Python.

A clean Ubuntu 24.04 export reproduced every pinned graph hash. The table uses the final packaged weights, which retained the candidate's per-object IoUs. A separate GPU profile with both SAM 3.1 and BiRefNet sessions retained measured 1.85 seconds for object-model session loading, 1.65 seconds for the first image prompt, and 84 ms for a cached prompt. All detected convolution, matrix multiplication and attention operators ran on the GPU; some decoder operations ran on the CPU.

The UnSAMv2+ export loads the official checkpoint with `weights_only=True` and the original builder, including its LoRA layers. Its encoder and decoder total approximately 76 MiB in FP16. Verification against the original PyTorch implementation covers two images, positive clicks, boxes, positive/negative clicks, granularity 0.3/0.7, and initial/refinement passes:

- FP32 ONNX had identical mask signs throughout; maximum absolute logit difference was `7.25e-5`.
- FP16 ONNX retained at least 99.9649% mask-sign agreement. An exporter conversion issue required correcting explicit Cast destinations to match FP16 tensor types before validation.
- Native CPU and Vulkan runs at fixed granularity 1.0 averaged 58.65% and 59.42% IoU respectively. Per-image binary agreement stayed above 99.6085%; the largest individual object IoU difference was 5.15 percentage points. Backend rounding affects boundaries, but does not explain the part/whole failures.

These checks validate the tested SAM 3.1 and UnSAM export paths. They do not establish pixel-identical output for every model or prompt combination.

### Reproducing the measurements

Model repositories and immutable revisions:

| Artifact | Source revision |
|---|---|
| SAM 2.1 Small ONNX | `onnx-community/sam2.1-hiera-small-ONNX@a7df49d8de14b9d2e4504d1687b0d568f905fd8d` |
| SAM 2.1 Large ONNX | `onnx-community/sam2.1-hiera-large-ONNX@3c23431f721e69cae82dbfd0c28fd692cc714021` |
| SAM 3 tracker ONNX | `onnx-community/sam3-tracker-ONNX@429305c8a5b3de597243d919a07e4e6bdcd00ef7` |
| SAM 3.1 checkpoint | `Comfy-Org/sam3.1@f38cd62b71494b53ac2b56ca36e24f3c8d565581`, `checkpoints/sam3.1_multiplex_fp16.safetensors` |
| UnSAMv2 source | `yujunwei04/UnSAMv2@2c9db1fd7da3c17590358eeb8c110231bd6a0889` |
| UnSAMv2+ checkpoint | `yujunwei04/UnSAMv2@2c5ab6ab513c47d170f564ae25412a318997ae24`, `unsamv2_plus.pt` |
| DAVIS archive | `datasets/yinloonga/DAVIS_2017@d13449d99b45add1bef7324db70e40ef27e707c5`, `DAVIS-2017-trainval-480p.zip` |

Fixture preparation uses frame `00000`, includes instance IDs other than 0/255, and computes max-exclusive boxes. For clicks, repeatedly erode each mask with a 3×3 square until the next erosion would be empty; choose the surviving pixel closest to the survivors' centroid, breaking ties in row-major order. The generated `cases.json` records image/mask paths, coordinates, and bounds. Local research artifacts are under `/tmp/compositor-instance-eval`; they are not bundled application assets.

SHA-256 identifiers:

```text
DAVIS archive       e3d0b5b77c3d031b000a19e0e25e3e2cac65d183755601bc2cf066df1a2aa492
cases.json          857427d1841a3e86d58b7d142c2af27bf5c328b165f3bf89ec2ff1e641876504
provenance.json     cf7d28095f35543911017ba6118e99a84dc4529b9704004edb3b88784f84c220
unsamv2_plus.pt     4067a6966b06df984828f537da0f02389ff28655a8985a1e1f4a3e1de4077195
encoder.fp16.onnx   15e0a0da12ae3dffe5ba26d7d1dd0a40475eee36cf261c604eff71213159c216
decoder.fp16.onnx   ebfba1e832af59cd2ffa9d0f0cb72ea0cde7de592cf643cb9aa8bac108328239
export_unsam.py     74a3f06cda31bba2ba7b1a43259a618c003b9595fad09094ebe8ca8573ecad46
verify_unsam.py     e5a8daf6ed39955ecd98672e89860ccc31eedd8835e07a6402ac6e5279554d03
```

`provenance.json` also records SHA-256 hashes of all ten extracted image/annotation files. The export used CPU PyTorch 2.14.0, the legacy ONNX exporter at opset 17, and FP16 conversion with public FP32 inputs/outputs. Python was used for research/export only; the benchmark and app inference are Rust/native.

Exact UnSAM native command used, from the repository root:

```sh
COMPOSITOR_INFERENCE_DIR=/tmp/appimage_extracted_7662e82fbdcdd2515a5e2a68d97d03d3/usr/share/compositor/inference \
COMPOSITOR_BACKGROUND_DEVICE=gpu \
COMPOSITOR_SEGMENTATION_MODEL="$HOME/.cache/compositor/inference-downloads/unsamv2-plus" \
COMPOSITOR_SEGMENTATION_CASES=/tmp/compositor-instance-eval/cases.json \
COMPOSITOR_SEGMENTATION_OUTPUT=/tmp/compositor-instance-eval/unsam-point-10 \
COMPOSITOR_UNSAM_GRANULARITY=1 \
cargo test --lib benchmark_unsam_objects -- --ignored --nocapture
```

Use the inference directory from a locally extracted AppImage when the recorded temporary path is unavailable. Set granularity to `0.5` for the other fixed setting, add `COMPOSITOR_UNSAM_REFINE=1` for a second pass, or `COMPOSITOR_SEGMENTATION_PROMPT=box` for boxes. Choose a separate output directory for each run. Set the device to `cpu` for backend comparison.

For SAM models, use the same runtime, device, fixture and output variables, change the model directory to `sam2.1-small`, `sam2.1-large`, `sam3-tracker`, or `sam3.1-tracker`, and run `cargo test --lib benchmark_labeled_objects -- --ignored --nocapture`. Set `COMPOSITOR_SEGMENTATION_ARCH=sam3` for SAM 3 and 3.1. Omit UnSAM-specific variables. Each run writes per-object mask PNGs and `results.json`.

The selected SAM 3.1 build inputs, generated graph hashes, and license are pinned in [`scripts/object-selection-model.json`](../scripts/object-selection-model.json). [`scripts/setup-object-selection.sh`](../scripts/setup-object-selection.sh) performs the reproducible CPU export with [`scripts/requirements-object-model-build.txt`](../scripts/requirements-object-model-build.txt); `--check` verifies an existing installation without exporting or downloading.
