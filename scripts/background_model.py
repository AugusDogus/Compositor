#!/usr/bin/env python3
"""Build-time ONNX lowering for the full BiRefNet GPU model.

GridSample performs the same bilinear, zero-padded sampling as DeformConv.
Folding kernel positions into the grid width avoids four GatherND tensors
and CPU-only reductions. The trained weights and output activation stay intact.
This tool is never shipped or invoked by the desktop application.
"""

import sys
import numpy as np
import onnx
from onnx import helper as h, numpy_helper as nh, TensorProto as T


def lower(m: onnx.ModelProto) -> onnx.ModelProto:
    result = []
    for original in m.graph.node:
        if original.op_type != "DeformConv":
            result.append(original)
            continue
        attrs = {a.name: h.get_attribute_value(a) for a in original.attribute}
        kh, kw = attrs["kernel_shape"]
        k = kh * kw
        og = attrs.get("offset_group", 1)
        group = attrs.get("group", 1)
        dh, dw = attrs.get("dilations", [1, 1])
        sh, sw = attrs.get("strides", [1, 1])
        pt, pl, _, _ = attrs.get("pads", [0, 0, 0, 0])
        x, w, offset = original.input[:3]
        bias = original.input[3] if len(original.input) > 3 else ""
        mask = original.input[4] if len(original.input) > 4 else ""
        count = 0

        def name(label):
            nonlocal count
            count += 1
            return original.name + "/vulkan_" + label + "_" + str(count)

        def const(value, dtype=np.int64):
            n = name("constant")
            m.graph.initializer.append(nh.from_array(np.asarray(value, dtype=dtype), n))
            return n

        def op(kind, *args, **attrs):
            n = name(kind)
            result.append(h.make_node(kind, list(args), [n], name=n, **attrs))
            return n

        def shape(*values):
            return op(
                "Concat",
                *[const([v]) if isinstance(v, int) else v for v in values],
                axis=0,
            )

        def reshape(t, *values):
            return op("Reshape", t, shape(*values))

        def dim(t, i):
            return op("Gather", t, const([i]), axis=0)

        def scalar(t):
            return op("Squeeze", t, const([0]))

        xs = op("Shape", x)
        os = op("Shape", offset)
        ws = op("Shape", w)
        n, c, ih, iw = [dim(xs, i) for i in range(4)]
        oh, ow = dim(os, 2), dim(os, 3)
        ng = op("Mul", n, const([og]))
        cg = op("Div", c, const([og]))
        ck = op("Mul", c, const([k]))
        wk = op("Mul", ow, const([k]))
        offsets = reshape(op("Cast", offset, to=T.FLOAT), n, og, k, 2, oh, ow)
        offsets = op("Transpose", offsets, perm=[0, 1, 4, 2, 5, 3])
        dy = op("Gather", offsets, const([0]), axis=5)
        dx = op("Gather", offsets, const([1]), axis=5)
        xr = op("Cast", op("Range", const(0), scalar(ow), const(1)), to=T.FLOAT)
        yr = op("Cast", op("Range", const(0), scalar(oh), const(1)), to=T.FLOAT)
        xb = op("Sub", op("Mul", xr, const(sw, np.float32)), const(pl, np.float32))
        yb = op("Sub", op("Mul", yr, const(sh, np.float32)), const(pt, np.float32))
        kx = const(
            (np.tile(np.arange(kw), kh) * dw).reshape(1, 1, 1, k, 1, 1), np.float32
        )
        ky = const(
            (np.repeat(np.arange(kh), kw) * dh).reshape(1, 1, 1, k, 1, 1), np.float32
        )
        xx = op("Add", dx, op("Add", reshape(xb, 1, 1, 1, 1, -1, 1), kx))
        yy = op("Add", dy, op("Add", reshape(yb, 1, 1, -1, 1, 1, 1), ky))

        def normalized(v, size):
            return op(
                "Sub",
                op(
                    "Div",
                    op("Add", op("Mul", v, const(2, np.float32)), const(1, np.float32)),
                    op("Cast", size, to=T.FLOAT),
                ),
                const(1, np.float32),
            )

        grid = op("Concat", normalized(xx, iw), normalized(yy, ih), axis=5)
        grid = reshape(grid, ng, oh, wk, 2)
        sampled = op(
            "GridSample",
            reshape(x, ng, cg, ih, iw),
            grid,
            align_corners=0,
            mode="bilinear",
            padding_mode="zeros",
        )
        sampled = reshape(sampled, n, og, cg, oh, k, ow)
        if mask:
            mask = reshape(mask, n, og, k, oh, ow)
            mask = op("Transpose", mask, perm=[0, 1, 3, 2, 4])
            mask = op("Unsqueeze", mask, const([2]))
            sampled = op("Mul", sampled, mask)
        sampled = op("Transpose", sampled, perm=[0, 1, 2, 4, 3, 5])
        sampled = reshape(sampled, n, ck, oh, ow)
        weight = reshape(w, dim(ws, 0), -1, 1, 1)
        inputs = [sampled, weight] + ([bias] if bias else [])
        result.append(
            h.make_node(
                "Conv",
                inputs,
                list(original.output),
                name=original.name + "/vulkan_projection",
                kernel_shape=[1, 1],
                group=group,
            )
        )
    del m.graph.node[:]
    m.graph.node.extend(result)
    return m


if __name__ == "__main__":
    source = onnx.load(sys.argv[1])
    onnx.checker.check_model(source)
    if sum(node.op_type == "DeformConv" for node in source.graph.node) != 20:
        raise ValueError(
            "Expected the pinned BiRefNet model with 20 deformable convolutions"
        )
    model = lower(source)
    onnx.checker.check_model(model)
    # Fold constant shape calculations once during packaging instead of each load.
    import onnxsim

    model, checked = onnxsim.simplify(model)
    if not checked:
        raise ValueError("BiRefNet graph simplification failed validation")
    onnx.checker.check_model(model)
    onnx.save(model, sys.argv[2])
    print("saved", sys.argv[2])
