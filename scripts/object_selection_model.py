#!/usr/bin/env python3
"""Export the pinned SAM 3.1 interactive tracker for native ONNX inference.

Build-time only. The AppImage contains ONNX graphs and weights, never Python.
The checkpoint also contains concept/video networks. Only the shared backbone,
interactive FPN, interactive prompt encoder, and interactive mask decoder are used.
"""

import argparse
import gc
import hashlib
import re
import tempfile
from pathlib import Path
from typing import Literal

import numpy as np
import onnx
import onnxruntime as ort
import torch
from onnxconverter_common import float16
from safetensors import safe_open
from torch import nn
from transformers import Sam3TrackerConfig, Sam3TrackerModel

CHECKPOINT_SHA256 = "9ba99c92703c2e8b4f47de2d34a539bb8e18923049e238b780d70dbe6368eb03"
CONFIG_SHA256 = "c3de7457775d88dce15e7451e1f035e5f634d7ce2a9c38a8c04b7915e7482d12"
FEATURE_NAMES = ["image_embeddings.0", "image_embeddings.1", "image_embeddings.2"]
PROMPT_NAMES = ["input_points", "input_labels", "input_boxes"]
OUTPUT_NAMES = ["pred_masks", "iou_scores", "object_score_logits"]


def verify_source(path: Path, expected: str) -> None:
    with path.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    if digest != expected:
        raise ValueError(
            f"{path.name} has checksum {digest}, expected {expected}. Download the pinned model source again."
        )


def source_key(key: str) -> tuple[str, int | Literal["pos", "points"] | None]:
    if key == "no_memory_embedding":
        return "tracker.model.interactivity_no_mem_embed", None
    if key in [
        "shared_image_embedding.positional_embedding",
        "prompt_encoder.shared_embedding.positional_embedding",
    ]:
        return (
            "tracker.model.interactive_sam_prompt_encoder.pe_layer.positional_encoding_gaussian_matrix",
            None,
        )
    if key.startswith("vision_encoder.backbone."):
        key = key.removeprefix("vision_encoder.backbone.")
        key = (
            key.replace("embeddings.position_embeddings", "pos_embed")
            .replace("embeddings.patch_embeddings.projection.", "patch_embed.proj.")
            .replace("layer_norm.", "ln_pre.")
        )
        key = re.sub(r"layers\.(\d+)\.layer_norm([12])\.", r"blocks.\1.norm\2.", key)
        key = re.sub(r"layers\.(\d+)\.attention\.", r"blocks.\1.attn.", key)
        key = re.sub(r"layers\.(\d+)\.mlp\.", r"blocks.\1.mlp.", key)
        key = key.replace(".o_proj.", ".proj.")
        for i, q in enumerate(["q", "k", "v"]):
            if f".{q}_proj." in key:
                return "detector.backbone.vision_backbone.trunk." + key.replace(
                    f".{q}_proj.", ".qkv."
                ), i
        return "detector.backbone.vision_backbone.trunk." + key, (
            "pos" if key == "pos_embed" else None
        )
    if key.startswith("vision_encoder.neck."):
        key = key.removeprefix("vision_encoder.neck.fpn_layers.")
        index = key.split(".")[0]
        key = (
            key.replace(
                "scale_layers.0.", "dconv_2x2_0." if index == "0" else "dconv_2x2."
            )
            .replace("scale_layers.2.", "dconv_2x2_1.")
            .replace("proj1.", "conv_1x1.")
            .replace("proj2.", "conv_3x3.")
        )
        return "detector.backbone.vision_backbone.interactive_convs." + key, None
    if key.startswith("prompt_encoder."):
        key = key.removeprefix("prompt_encoder.")
        for a, b in [
            ("mask_embed.conv1.", "mask_downscaling.0."),
            ("mask_embed.conv2.", "mask_downscaling.3."),
            ("mask_embed.conv3.", "mask_downscaling.6."),
            ("mask_embed.layer_norm1.", "mask_downscaling.1."),
            ("mask_embed.layer_norm2.", "mask_downscaling.4."),
        ]:
            key = key.replace(a, b)
        if key == "point_embed.weight":
            return (
                "tracker.model.interactive_sam_prompt_encoder.point_embeddings.",
                "points",
            )
        return "tracker.model.interactive_sam_prompt_encoder." + key, None
    if key.startswith("mask_decoder."):
        key = key.removeprefix("mask_decoder.")
        key = key.replace(".layer_norm", ".norm").replace(".o_proj.", ".out_proj.")
        for a, b in [
            ("upscale_conv1.", "output_upscaling.0."),
            ("upscale_conv2.", "output_upscaling.3."),
            ("upscale_layer_norm.", "output_upscaling.1."),
        ]:
            key = key.replace(a, b)
        if key.startswith("transformer."):
            key = key.replace(".mlp.proj_in.", ".mlp.lin1.").replace(
                ".mlp.proj_out.", ".mlp.lin2."
            )
        else:
            # Original three-layer MLP -> HF named input/output + middle layer.
            key = (
                key.replace(".layers.0.", ".layers.1.")
                .replace(".proj_in.", ".layers.0.")
                .replace(".proj_out.", ".layers.2.")
            )
        return "tracker.model.interactive_sam_mask_decoder." + key, None
    raise KeyError(key)


def load_model(checkpoint: Path, config_path: Path) -> Sam3TrackerModel:
    verify_source(checkpoint, CHECKPOINT_SHA256)
    verify_source(config_path, CONFIG_SHA256)
    config = Sam3TrackerConfig.from_json_file(str(config_path))
    # The 3.1 interactive neck has three scales. SAM3's unused fourth scale
    # belongs to the concept model and must not receive invented weights.
    config.vision_config.scale_factors = [4.0, 2.0, 1.0]
    config._attn_implementation = "eager"
    config.vision_config._attn_implementation = "eager"
    config.vision_config.backbone_config._attn_implementation = "eager"
    model = Sam3TrackerModel(config)
    state = {}
    with safe_open(str(checkpoint), framework="pt", device="cpu") as source:
        for key, expected in model.state_dict().items():
            source_name, part = source_key(key)
            if part == "points":
                tensor = torch.cat(
                    [
                        source.get_tensor(source_name + str(i) + ".weight")
                        for i in range(4)
                    ],
                    dim=0,
                )
            else:
                tensor = source.get_tensor(source_name)
                if part == "pos":
                    tensor = tensor[:, 1:]
                elif part is not None:
                    tensor = tensor.chunk(3, dim=0)[part]
            if tensor.shape != expected.shape:
                raise ValueError(
                    f"{source_name} maps to {key} with shape {tuple(tensor.shape)}, expected {tuple(expected.shape)}. The pinned architecture no longer matches."
                )
            state[key] = tensor.float().contiguous()
    model.load_state_dict(state, strict=True, assign=True)
    # HF's SAM3 adapter hardcodes sigmoid, but official 3.1 exposes raw IoU
    # scores. Keep the official behavior, including scores outside [0,1].
    model.mask_decoder.iou_prediction_head.sigmoid_output = False
    return model.eval()


class Encoder(nn.Module):
    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, pixel_values):
        return tuple(self.model.get_image_embeddings(pixel_values))


class Decoder(nn.Module):
    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, input_points, input_labels, input_boxes, f0, f1, f2):
        corners = input_boxes.reshape(1, 1, -1, 2)
        corner_labels = (
            torch.tensor([2, 3], dtype=torch.int64)
            .repeat(input_boxes.shape[1])
            .reshape(1, 1, -1)
        )
        points = torch.cat([input_points, corners], dim=2)
        labels = torch.cat([input_labels, corner_labels], dim=2)
        sparse = self.model.prompt_encoder._embed_points(points, labels, pad=True)
        dense = self.model.prompt_encoder.no_mask_embed.weight.reshape(
            1, -1, 1, 1
        ).expand(1, -1, 72, 72)
        masks, scores, _, obj = self.model.mask_decoder(
            image_embeddings=f2,
            image_positional_embeddings=self.model.get_image_wide_positional_embeddings(),
            sparse_prompt_embeddings=sparse,
            dense_prompt_embeddings=dense,
            multimask_output=True,
            high_resolution_features=[f0, f1],
        )
        return masks, scores, obj


def convert_half(source: Path, destination: Path) -> None:
    graph = onnx.load(str(source))
    graph = onnx.shape_inference.infer_shapes(graph)

    def adapt_casts(current: onnx.GraphProto) -> None:
        for node in current.node:
            if node.op_type == "Cast":
                for attribute in node.attribute:
                    if attribute.name == "to" and attribute.i == onnx.TensorProto.FLOAT:
                        attribute.i = onnx.TensorProto.FLOAT16
            for attribute in node.attribute:
                if attribute.type == onnx.AttributeProto.GRAPH:
                    adapt_casts(attribute.g)

    # The converter changes tensor types but retains source Cast destinations.
    # Fix them first: its redundant-cast pass otherwise erases the required
    # public-input Cast16 followed by an original Cast32. Converter-added
    # public-output casts remain Float32.
    adapt_casts(graph.graph)
    graph = float16.convert_float_to_float16(
        graph, keep_io_types=True, disable_shape_infer=True, check_fp16_ready=False
    )
    data = destination.with_name(destination.name + "_data")
    data.unlink(missing_ok=True)
    onnx.save_model(
        graph,
        str(destination),
        save_as_external_data=True,
        all_tensors_to_one_file=True,
        location=data.name,
        size_threshold=1024,
    )
    del graph
    gc.collect()


def session(path: Path) -> ort.InferenceSession:
    options = ort.SessionOptions()
    options.intra_op_num_threads = 4
    options.inter_op_num_threads = 1
    return ort.InferenceSession(
        str(path), sess_options=options, providers=["CPUExecutionProvider"]
    )


def verify_export(
    output: Path, sample: torch.Tensor, features: tuple, decoder: Decoder
) -> None:
    encoder = session(output / "vision_encoder_fp16.onnx")
    encoded = encoder.run(None, {"pixel_values": sample.numpy()})
    for actual, expected in zip(encoded, features, strict=True):
        if actual.shape != tuple(expected.shape) or not np.isfinite(actual).all():
            raise ValueError(
                "Exported encoder returned an invalid feature tensor. Model installation was not completed."
            )
    del encoder
    gc.collect()
    exported = session(output / "prompt_encoder_mask_decoder_fp16.onnx")
    # Exercise dynamic empty points/boxes, multiple clicks, and combined prompts.
    for point_count, box_count in [(1, 0), (0, 1), (2, 0), (1, 1)]:
        points = torch.full((1, 1, point_count, 2), 400.0)
        labels = torch.ones((1, 1, point_count), dtype=torch.int64)
        boxes = (
            torch.tensor([100.0, 100.0, 700.0, 700.0])
            .repeat(box_count)
            .reshape(1, box_count, 4)
        )
        inputs = dict(zip(FEATURE_NAMES, encoded, strict=True))
        inputs.update(
            dict(
                zip(
                    PROMPT_NAMES,
                    [points.numpy(), labels.numpy(), boxes.numpy()],
                    strict=True,
                )
            )
        )
        outputs = exported.run(OUTPUT_NAMES, inputs)
        expected = decoder(points, labels, boxes, *features)
        for name, actual, reference in zip(
            OUTPUT_NAMES, outputs, expected, strict=True
        ):
            if actual.shape != tuple(reference.shape) or not np.isfinite(actual).all():
                raise ValueError(
                    f"Exported {name} is invalid for {point_count} points and {box_count} boxes. Model installation was not completed."
                )
            # FP16 may move logits near zero; reject large export errors while
            # allowing expected rounding on this deterministic smoke fixture.
            reference = reference.numpy()
            relative = np.linalg.norm(actual - reference) / max(
                float(np.linalg.norm(reference)), 1e-6
            )
            if relative > 0.1:
                raise ValueError(
                    f"Exported {name} differs from the mapped model by {relative:.3%}. Model installation was not completed."
                )


def export(checkpoint: Path, config: Path, output: Path) -> None:
    torch.set_num_threads(4)
    output.mkdir(parents=True, exist_ok=True)
    model = load_model(checkpoint, config)
    encoder, decoder = Encoder(model).eval(), Decoder(model).eval()
    sample = torch.zeros(1, 3, 1008, 1008)
    with (
        torch.inference_mode(),
        tempfile.TemporaryDirectory(prefix=".sam31-export-", dir=output) as directory,
    ):
        staging = Path(directory)
        print("Computing SAM 3.1 export reference", flush=True)
        features = encoder(sample)
        prompts = (
            torch.tensor([[[[400.0, 400.0]]]]),
            torch.ones(1, 1, 1, dtype=torch.int64),
            torch.zeros(1, 0, 4),
        )
        print("Exporting SAM 3.1 encoder", flush=True)
        torch.onnx.export(
            encoder,
            (sample,),
            str(staging / "encoder.onnx"),
            input_names=["pixel_values"],
            output_names=FEATURE_NAMES,
            opset_version=17,
            dynamo=False,
        )
        convert_half(staging / "encoder.onnx", output / "vision_encoder_fp16.onnx")
        print("Exporting SAM 3.1 decoder", flush=True)
        torch.onnx.export(
            decoder,
            (*prompts, *features),
            str(staging / "decoder.onnx"),
            input_names=PROMPT_NAMES + FEATURE_NAMES,
            output_names=OUTPUT_NAMES,
            dynamic_axes={
                "input_points": {2: "num_points_per_image"},
                "input_labels": {2: "num_points_per_image"},
                "input_boxes": {1: "num_boxes_per_image"},
            },
            opset_version=17,
            dynamo=False,
        )
        convert_half(
            staging / "decoder.onnx", output / "prompt_encoder_mask_decoder_fp16.onnx"
        )
        print("Verifying SAM 3.1 native graphs", flush=True)
        # The decoder only needs prompt/mask modules. Release the large vision
        # backbone before ORT allocates its verification session on CI workers.
        del model.vision_encoder
        gc.collect()
        verify_export(output, sample, features, decoder)
    print("SAM 3.1 export verified", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    export(args.checkpoint, args.config, args.output)


if __name__ == "__main__":
    main()
