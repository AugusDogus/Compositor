#!/usr/bin/env bash
# Shared native architecture names and checksum-pinned packaging/runtime artifacts.
compositor_linux_architecture() {
    case "$1" in
        x86_64)
            linux_arch=x86_64
            linux_deb_arch=amd64
            linux_multiarch=x86_64-linux-gnu
            linux_elf_machine=62
            linux_ldconfig_tag=x86-64
            linux_ort_arch=x64
            linux_ort_sha256=a5ed5a3cac51fbb2e90da632ae43d19212faaa20e76484e62bcb7c23ddb3b3fd
            linux_webgpu_version=0.3.0
            linux_webgpu_url=https://files.pythonhosted.org/packages/97/9c/d37bc05c56c3d91d44585db7bebbf0f068ece5d01df5b3898449771d4bf2/onnxruntime_ep_webgpu-0.3.0-py3-none-manylinux_2_28_x86_64.whl
            linux_webgpu_sha256=865ce82d80319d7f259a4a65e66e32834f0f117db55ae4377868b4f28016e7bf
            linux_deploy_sha256=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
            linux_appimagetool_sha256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
            linux_runtime_sha256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
            ;;
        aarch64|arm64)
            linux_arch=aarch64
            linux_deb_arch=arm64
            linux_multiarch=aarch64-linux-gnu
            linux_elf_machine=183
            linux_ldconfig_tag=AArch64
            linux_ort_arch=aarch64
            linux_ort_sha256=e16a27a8ed330bbc698df7330b0cf56e722f354e3bcc92118682c74ef3c3e3da
            # 0.4.0 is the first published Linux ARM64 plugin. It accepts ORT >=1.24.4.
            linux_webgpu_version=0.4.0
            linux_webgpu_url=https://files.pythonhosted.org/packages/4e/ca/00c70322c19913c81a6bb2239aca83781b97a818b55c2ec3d25cec72dc4c/onnxruntime_ep_webgpu-0.4.0-py3-none-manylinux_2_28_aarch64.whl
            linux_webgpu_sha256=17f660db53b1a509c63e721ac6784a2daa1c4e91a968523ee9986505ee5b0a55
            linux_deploy_sha256=620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff
            linux_appimagetool_sha256=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
            linux_runtime_sha256=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444
            ;;
        *) printf 'Unsupported Linux architecture: %s. Build on x86_64 or aarch64 Linux.\n' "$1" >&2; return 1 ;;
    esac
}

# Check ELF headers without executing the input or trusting its filename. This
# also detects a stale native binary passed to a build on another architecture.
compositor_check_linux_binary() {
    python3 - "$1" "$linux_elf_machine" "$linux_arch" <<'PY'
import pathlib, struct, sys
path, machine, arch = sys.argv[1:]
with pathlib.Path(path).open('rb') as binary:
    header = binary.read(64)
if (len(header) != 64 or header[:4] != b'\x7fELF' or header[4:6] != b'\x02\x01'
        or struct.unpack_from('<H', header, 18)[0] != int(machine)):
    sys.exit(f'{path} is not a 64-bit little-endian Linux {arch} ELF binary. Rebuild for the packaging host.')
PY
}
