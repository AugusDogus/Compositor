import numpy as np
import onnx
import onnxruntime as ort
from onnx import helper as h, numpy_helper as nh, TensorProto as T
from background_model import lower

rng = np.random.default_rng(7)
for kh, kw, group, og, stride, dilation, pad in [
    (1, 1, 1, 1, 1, 1, 0),
    (3, 3, 1, 1, 1, 1, 1),
    (3, 2, 2, 3, 2, 2, 2),
    (7, 7, 1, 1, 1, 1, 3),
]:
    n, c, ih, iw, m = 2, 6, 9, 7, 8
    oh = (ih + 2 * pad - dilation * (kh - 1) - 1) // stride + 1
    ow = (iw + 2 * pad - dilation * (kw - 1) - 1) // stride + 1
    data = {
        "x": rng.normal(size=(n, c, ih, iw)).astype("float32"),
        "offset": rng.uniform(-2, 2, size=(n, 2 * og * kh * kw, oh, ow)).astype(
            "float32"
        ),
        "mask": rng.uniform(size=(n, og * kh * kw, oh, ow)).astype("float32"),
    }
    weights = rng.normal(size=(m, c // group, kh, kw)).astype("float32")
    bias = rng.normal(size=(m,)).astype("float32")
    node = h.make_node(
        "DeformConv",
        ["x", "w", "offset", "b", "mask"],
        ["y"],
        name="deform",
        kernel_shape=[kh, kw],
        group=group,
        offset_group=og,
        strides=[stride, stride],
        dilations=[dilation, dilation],
        pads=[pad] * 4,
    )
    graph = h.make_graph(
        [node],
        "check",
        [h.make_tensor_value_info(k, T.FLOAT, v.shape) for k, v in data.items()],
        [h.make_tensor_value_info("y", T.FLOAT, [n, m, oh, ow])],
        [nh.from_array(weights, "w"), nh.from_array(bias, "b")],
    )
    model = h.make_model(graph, opset_imports=[h.make_opsetid("", 19)], ir_version=10)
    options = ort.SessionOptions()
    options.intra_op_num_threads = 4
    expected = ort.InferenceSession(
        model.SerializeToString(), options, providers=["CPUExecutionProvider"]
    ).run(None, data)[0]
    actual = ort.InferenceSession(
        lower(model).SerializeToString(), options, providers=["CPUExecutionProvider"]
    ).run(None, data)[0]
    np.testing.assert_allclose(actual, expected, rtol=2e-4, atol=5e-5)
    print("passed", kh, kw, group, og, "maxdiff", np.max(np.abs(actual - expected)))
