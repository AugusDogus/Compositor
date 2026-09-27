"""FP32 CPU arithmetic with the GPU graph's existing FP16 weight storage."""

from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper


def cpu_graph(source: Path, destination: Path) -> None:
    if source.parent != destination.parent:
        raise ValueError("CPU and GPU graphs must share their external weight directory")
    model = onnx.load(source, load_external_data=False)

    def promote(graph: onnx.GraphProto) -> None:
        for value in [*graph.input, *graph.output, *graph.value_info]:
            tensor = value.type.tensor_type
            if tensor.elem_type == TensorProto.FLOAT16:
                tensor.elem_type = TensorProto.FLOAT
        names = {
            name for node in graph.node for name in [*node.input, *node.output]
        } | {value.name for value in graph.initializer}
        casts = []
        for weight in graph.initializer:
            if weight.data_type != TensorProto.FLOAT16:
                continue
            original = weight.name
            stored = original + "_cpu_fp16"
            if stored in names:
                raise ValueError(f"CPU weight name collides with graph value: {stored}")
            names.add(stored)
            weight.name = stored
            casts.append(
                helper.make_node("Cast", [stored], [original], to=TensorProto.FLOAT)
            )
        for node in graph.node:
            for attribute in node.attribute:
                if (
                    node.op_type == "Cast"
                    and attribute.name == "to"
                    and attribute.i == TensorProto.FLOAT16
                ):
                    attribute.i = TensorProto.FLOAT
                elif attribute.type == onnx.AttributeProto.GRAPH:
                    promote(attribute.g)
                elif (
                    attribute.type == onnx.AttributeProto.TENSOR
                    and attribute.t.data_type == TensorProto.FLOAT16
                ):
                    tensor = attribute.t
                    values = numpy_helper.to_array(tensor, base_dir=str(source.parent))
                    tensor.CopyFrom(
                        numpy_helper.from_array(values.astype(np.float32), tensor.name)
                    )
        nodes = list(graph.node)
        del graph.node[:]
        graph.node.extend([*casts, *nodes])

    promote(model.graph)
    onnx.save(model, destination)
    onnx.checker.check_model(str(destination))
