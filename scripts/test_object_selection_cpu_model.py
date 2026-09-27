"""Verify CPU arithmetic and shared GPU weight storage without SAM downloads."""

import hashlib
from pathlib import Path
import tempfile
import unittest

import numpy as np
import onnx
import onnxruntime as ort
from onnx import TensorProto as T, helper as h, numpy_helper as nh

from object_selection_cpu_model import cpu_graph


class CpuGraphTests(unittest.TestCase):
    def test_cpu_accumulation_avoids_half_overflow_and_reuses_weights(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, destination = root / "gpu.onnx", root / "cpu.onnx"
            graph = h.make_graph(
                [h.make_node("Cast", ["x"], ["half"], to=T.FLOAT16),
                 h.make_node("MatMul", ["half", "w"], ["product"]),
                 h.make_node("Cast", ["product"], ["y"], to=T.FLOAT)],
                "shared-weights",
                [h.make_tensor_value_info("x", T.FLOAT, [1, 2])],
                [h.make_tensor_value_info("y", T.FLOAT, [1, 1])],
                [nh.from_array(np.full((2, 1), 65504, dtype=np.float16), "w")],
                value_info=[h.make_tensor_value_info("product", T.FLOAT16, [1, 1])],
            )
            model = h.make_model(graph, opset_imports=[h.make_opsetid("", 17)], ir_version=10)
            onnx.save_model(model, source, save_as_external_data=True,
                            all_tensors_to_one_file=True, location="weights.bin", size_threshold=0)
            original = {path.name: hashlib.sha256(path.read_bytes()).digest() for path in root.iterdir()}
            cpu_graph(source, destination)
            for name, digest in original.items():
                self.assertEqual(hashlib.sha256((root / name).read_bytes()).digest(), digest)
            self.assertEqual({path.name for path in root.iterdir()}, {"gpu.onnx", "cpu.onnx", "weights.bin"})
            stored = onnx.load(destination, load_external_data=False).graph.initializer[0]
            self.assertEqual(stored.data_type, T.FLOAT16)
            self.assertEqual(stored.data_location, T.EXTERNAL)
            session = ort.InferenceSession(str(destination), providers=["CPUExecutionProvider"])
            actual = session.run(None, {"x": np.ones((1, 2), dtype=np.float32)})[0]
            np.testing.assert_array_equal(actual, [[131008]])


if __name__ == "__main__":
    unittest.main()
