#!/usr/bin/env bash
# Export pinned object-selection weights with CPU build tools; bundle native graphs only.
set -euo pipefail
[[ $# == 0 || ( $# == 1 && "$1" == --check ) ]] || { printf 'Usage: %s [--check]\n' "$0" >&2; exit 1; }
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
manifest="$script_dir/object-selection-model.json"
inference_dir="${COMPOSITOR_INFERENCE_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/compositor/inference}"
cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/compositor/inference-downloads"
for command in python3 sha256sum stat; do
    command -v "$command" >/dev/null || { printf 'Install %s and retry.\n' "$command" >&2; exit 1; }
done
if [[ "${1:-}" == --check ]]; then
    stage_dir="$(mktemp -d)"
else
    mkdir -p "$inference_dir"
    stage_dir="$(mktemp -d "$inference_dir/.object-setup.XXXXXX")"
fi
download=""
trap 'rm -rf -- "$stage_dir"; [[ -z "$download" ]] || rm -f -- "$download"' EXIT
python3 - "$manifest" "$stage_dir" <<'PY'
import json
import pathlib
import re
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
stage = pathlib.Path(sys.argv[2])
identifier = manifest.get("id") if isinstance(manifest, dict) else None
if not isinstance(identifier, str) or not re.fullmatch(r"[a-z0-9][a-z0-9.-]*", identifier):
    sys.exit("Object-selection manifest needs a valid model id.")

def entry(item, pattern, seen, downloaded):
    if not isinstance(item, dict):
        sys.exit("Object-selection manifest contains an invalid file entry.")
    path, checksum, size = (item.get(key) for key in ("path", "sha256", "bytes"))
    if not isinstance(path, str) or not re.fullmatch(pattern, path) or path in seen:
        sys.exit("Object-selection manifest contains an invalid or duplicate path.")
    if not isinstance(checksum, str) or not re.fullmatch(r"[a-f0-9]{64}", checksum):
        sys.exit(f"Object-selection manifest has an invalid checksum for {path}.")
    if type(size) is not int or size <= 0:
        sys.exit(f"Object-selection manifest has an invalid size for {path}.")
    url = item.get("url", "-")
    if downloaded and (not isinstance(url, str) or not re.fullmatch(r"https://(?:huggingface\.co/[^/]+/[^/]+/resolve|raw\.githubusercontent\.com/[^/]+/[^/]+)/[a-f0-9]{40}/[^\s]+", url)):
        sys.exit(f"Object-selection manifest needs a commit-pinned URL for {path}.")
    if not downloaded and url != "-":
        sys.exit(f"Generated object-selection file {path} must not have a download URL.")
    seen.add(path)
    return [identifier, path, checksum, str(size), url]

build = manifest.get("build")
if not isinstance(build, dict) or set(build) != {"checkpoint", "config"}:
    sys.exit("Object-selection manifest needs pinned checkpoint and config build inputs.")
with (stage / "inputs.tsv").open("w") as output:
    seen = set()
    for role in ("checkpoint", "config"):
        print(role, *entry(build[role], r"[A-Za-z0-9][A-Za-z0-9_.-]*", seen, True), sep="\t", file=output)
files = manifest.get("files")
if not isinstance(files, list) or not files:
    sys.exit("Object-selection manifest has no output files.")
with (stage / "files.tsv").open("w") as output:
    seen = set()
    for item in files:
        source = item.get("source") if isinstance(item, dict) else None
        if source not in ("build", "download"):
            sys.exit("Object-selection file source must be build or download.")
        row = entry(item, r"(?:object-selection|licenses)/[A-Za-z0-9][A-Za-z0-9_.-]*", seen, source == "download")
        print(source, *row, sep="\t", file=output)
PY

valid_file() {
    local path="$1" checksum="$2" size="$3"
    [[ -f "$path" && "$(stat -c %s "$path")" == "$size" ]] &&
        printf '%s  %s\n' "$checksum" "$path" | sha256sum --check --status
}
if [[ "${1:-}" == --check ]]; then
    while IFS=$'\t' read -r source model path checksum size url; do
        valid_file "$inference_dir/$path" "$checksum" "$size" || {
            printf 'Object-selection dependency %s is missing or differs from its pinned checksum. Rerun scripts/setup-background.sh or download a fresh AppImage.\n' "$path" >&2
            exit 1
        }
    done < "$stage_dir/files.tsv"
    cmp --silent "$manifest" "$inference_dir/object-selection/model-manifest.json" || {
        printf 'Bundled object-selection manifest does not match this source revision. Rebuild the AppImage.\n' >&2
        exit 1
    }
    exit 0
fi

command -v curl >/dev/null || { printf 'Install curl and retry.\n' >&2; exit 1; }
fetch() {
    local cache="$1" checksum="$2" size="$3" url="$4"
    mkdir -p "$(dirname "$cache")"
    if ! valid_file "$cache" "$checksum" "$size"; then
        download="$(mktemp "$(dirname "$cache")/.download.XXXXXX")"
        if ! curl --fail --location --retry 3 --output "$download" "$url" ||
            ! valid_file "$download" "$checksum" "$size"; then
            printf 'Could not download and verify %s. Installed models are unchanged; retry setup.\n' "$(basename "$cache")" >&2
            exit 1
        fi
        mv -Tf "$download" "$cache"
    fi
}
needs_build=false
while IFS=$'\t' read -r source model path checksum size url; do
    if [[ "$source" == build ]] && ! valid_file "$cache_dir/$model/$(basename "$path")" "$checksum" "$size"; then
        needs_build=true
    fi
done < "$stage_dir/files.tsv"
if [[ "$needs_build" == true ]]; then
    while IFS=$'\t' read -r role model path checksum size url; do
        fetch "$cache_dir/$model/$path" "$checksum" "$size" "$url"
        case "$role" in
            checkpoint) checkpoint="$cache_dir/$model/$path" ;;
            config) config="$cache_dir/$model/$path" ;;
        esac
    done < "$stage_dir/inputs.tsv"
    requirements="$script_dir/requirements-object-model-build.txt"
    requirements_hash="$(sha256sum "$requirements" | cut -c1-16)"
    python_version="$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')"
    build_env="$cache_dir/object-model-build-py$python_version-$requirements_hash"
    # Cached virtualenvs can come from another host/container interpreter. Recreate
    # incompatible environments rather than importing packages from that machine.
    base_prefix="$(python3 -c 'import sys; print(sys.base_prefix)')"
    cached_prefix="$("$build_env/bin/python" -c 'import sys; print(sys.base_prefix)' 2>/dev/null || true)"
    if [[ "$cached_prefix" != "$base_prefix" ]]; then
        python3 -m venv --clear "$build_env"
    fi
    "$build_env/bin/python" -m pip install --disable-pip-version-check --only-binary=:all: -r "$requirements"
    "$build_env/bin/python" -B "$script_dir/object_selection_model.py" \
        --checkpoint "$checkpoint" --config "$config" --output "$stage_dir/export"
    # Validate the entire export before replacing any cached or installed graph.
    while IFS=$'\t' read -r source model path checksum size url; do
        if [[ "$source" == build ]] && ! valid_file "$stage_dir/export/$(basename "$path")" "$checksum" "$size"; then
            printf 'Exported %s differs from its pinned output. Installed models are unchanged; check the pinned build dependencies and retry.\n' "$path" >&2
            exit 1
        fi
    done < "$stage_dir/files.tsv"
    while IFS=$'\t' read -r source model path checksum size url; do
        if [[ "$source" == build ]]; then
            mv -Tf "$stage_dir/export/$(basename "$path")" "$cache_dir/$model/$(basename "$path")"
        fi
    done < "$stage_dir/files.tsv"
fi
while IFS=$'\t' read -r source model path checksum size url; do
    cache="$cache_dir/$model/$(basename "$path")"
    if [[ "$source" == download ]]; then fetch "$cache" "$checksum" "$size" "$url"; fi
    mkdir -p "$stage_dir/$(dirname "$path")"
    cp --reflink=auto "$cache" "$stage_dir/$path"
done < "$stage_dir/files.tsv"
# Stage every verified graph and license before installing. Rename preserves files
# mapped by running sessions. Restart a source-build editor after updating models.
while IFS=$'\t' read -r source model path checksum size url; do
    mkdir -p "$inference_dir/$(dirname "$path")"
    if ! valid_file "$inference_dir/$path" "$checksum" "$size"; then
        chmod 644 "$stage_dir/$path"
        mv -Tf "$stage_dir/$path" "$inference_dir/$path"
    fi
done < "$stage_dir/files.tsv"
cp "$manifest" "$stage_dir/manifest.json"
chmod 644 "$stage_dir/manifest.json"
mv -Tf "$stage_dir/manifest.json" "$inference_dir/object-selection/model-manifest.json"
printf 'Native object-selection model installed in %s/object-selection.\n' "$inference_dir"
