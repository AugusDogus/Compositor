#!/usr/bin/env python3
"""Architecture boundaries and combined release feeds, without building packages."""
import json
import os
import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent


def architecture(arch, command, *arguments):
    return subprocess.run(
        ["bash", "-c", 'set -euo pipefail; source scripts/linux-architecture.sh; compositor_linux_architecture "$1"; shift; ' + command,
         "bash", arch, *map(str, arguments)],
        cwd=ROOT, capture_output=True, text=True,
    )


def feed(arch, version="1.2.3"):
    return {
        "version": version,
        "pub_date": "2026-09-27T00:00:00Z",
        "notes": "Release",
        "platforms": {f"linux-{arch}": {
            "url": f"https://github.com/AugusDogus/Compositor/releases/download/v{version}/Compositor-{version}-linux-{arch}.bin",
        }},
    }


class ArchitectureTests(unittest.TestCase):
    def test_debian_and_rust_platform_names_match(self):
        for arch, expected in [("x86_64", "x86_64 amd64"), ("aarch64", "aarch64 arm64"), ("arm64", "aarch64 arm64")]:
            with self.subTest(arch=arch):
                result = architecture(arch, 'printf "%s %s" "$linux_arch" "$linux_deb_arch"')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected)
        self.assertNotEqual(architecture("i686", ":").returncode, 0)

    def test_foreign_binary_and_malformed_elf_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            binary = pathlib.Path(directory) / "binary"
            for arch, machine in [("x86_64", 62), ("aarch64", 183)]:
                header = bytearray(64)
                header[:6] = b"\x7fELF\x02\x01"
                struct.pack_into("<H", header, 18, machine)
                binary.write_bytes(header)
                result = architecture(arch, 'compositor_check_linux_binary "$1"', binary)
                self.assertEqual(result.returncode, 0, result.stderr)
                foreign = "aarch64" if arch == "x86_64" else "x86_64"
                self.assertNotEqual(architecture(foreign, 'compositor_check_linux_binary "$1"', binary).returncode, 0)
                header[4] = 1
                binary.write_bytes(header)
                self.assertNotEqual(architecture(arch, 'compositor_check_linux_binary "$1"', binary).returncode, 0)
            binary.write_bytes(b"not an ELF binary")
            self.assertNotEqual(architecture("x86_64", 'compositor_check_linux_binary "$1"', binary).returncode, 0)


class AppImageTestDiscoveryTests(unittest.TestCase):
    def test_discovery_drains_output_and_preserves_failures(self):
        # Exercise the actual test-discovery block without extracting a multi-GB AppImage.
        script = (ROOT / "scripts/check-appimage.sh").read_text()
        checks = script[script.index('if [[ -n "${COMPOSITOR_IMAGE_IO_TEST_BINARY:-}" ]]'):]
        variables = ["COMPOSITOR_IMAGE_IO_TEST_BINARY", "COMPOSITOR_OBJECT_SELECTION_TEST_BINARY", "COMPOSITOR_RAW_TEST_BINARY"]
        with tempfile.TemporaryDirectory() as directory:
            binary = pathlib.Path(directory) / "test-binary"
            fixture = pathlib.Path(directory) / "fixture.raf"
            fixture.touch()
            for variable in variables:
                for mode in ["complete", "empty", "failed"]:
                    with self.subTest(variable=variable, mode=mode):
                        binary.write_text(
                            f"#!{sys.executable}\nimport sys, time\n"
                            "if '--list' in sys.argv:\n"
                            f"    if {mode!r} != 'empty':\n"
                            "        print('fixture: test', flush=True)\n"
                            "        time.sleep(0.05)\n"
                            "        sys.stdout.write('padding\\n' * 16384)\n"
                            "        sys.stdout.flush()\n"
                            f"    sys.exit(3 if {mode!r} == 'failed' else 0)\n"
                        )
                        binary.chmod(0o755)
                        env = os.environ.copy()
                        for key in [*variables, "COMPOSITOR_INFERENCE_TEST_BINARY"]:
                            env.pop(key, None)
                        env[variable] = str(binary)
                        env["COMPOSITOR_XTRANS_TEST_PHOTO"] = str(fixture)
                        result = subprocess.run(
                            ["bash", "-c", "set -euo pipefail\n" + checks],
                            env=env, capture_output=True, text=True, timeout=10,
                        )
                        if mode == "complete":
                            self.assertEqual(result.returncode, 0, result.stderr)
                        else:
                            self.assertNotEqual(result.returncode, 0)


class UpdateFeedTests(unittest.TestCase):
    def merge(self, first, second):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = pathlib.Path(directory.name)
        output = root / "linux-update.json"
        output.write_text("previous feed")
        inputs = [root / "first.json", root / "second.json"]
        for path, value in zip(inputs, [first, second]):
            path.write_text(json.dumps(value))
        result = subprocess.run(
            ["bash", str(ROOT / "scripts/merge-linux-updates.sh"), str(output), *map(str, inputs)],
            capture_output=True, text=True,
        )
        self.assertEqual(sorted(path.name for path in root.iterdir()), ["first.json", "linux-update.json", "second.json"])
        return result, output.read_text()

    def test_both_architectures_share_one_feed(self):
        result, output = self.merge(feed("x86_64"), feed("aarch64"))
        self.assertEqual(result.returncode, 0, result.stderr)
        merged = json.loads(output)
        self.assertEqual(merged["version"], "1.2.3")
        self.assertEqual(sorted(merged["platforms"]), ["linux-aarch64", "linux-x86_64"])

    def test_bad_inputs_preserve_previous_output(self):
        wrong_url = feed("aarch64")
        wrong_url["platforms"]["linux-aarch64"]["url"] = feed("x86_64")["platforms"]["linux-x86_64"]["url"]
        for second in [feed("x86_64"), feed("aarch64", "1.2.4"), feed("riscv64"), wrong_url, {}]:
            with self.subTest(second=second):
                result, output = self.merge(feed("x86_64"), second)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, "previous feed")


if __name__ == "__main__":
    unittest.main()
