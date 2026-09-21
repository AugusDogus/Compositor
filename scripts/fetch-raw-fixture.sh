#!/usr/bin/env bash
# Nikon D70, 12-bit lossy-compressed NEF, raw.pixls.us entry 2060.
# CC0 1.0: https://creativecommons.org/publicdomain/zero/1.0/
# Source metadata: https://raw.pixls.us/getfile.php/2060/exif/20170902_0047.NEF.exif.txt
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
destination=${COMPOSITOR_RAW_FIXTURE_DIR:-"$root/target/fixtures/raw-external"}
digest=dd6405aeb33b0cd5bf66c98ba98ccbb478a765450cfd130810e470dab8d1f4b4
mkdir -p -- "$destination"
filename="$destination/nikon-d70.nef"
if ! { [[ -f "$filename" ]] && printf '%s  %s\n' "$digest" "$filename" | sha256sum --check --status; }; then
    staging=$(mktemp "$destination/.download-XXXXXX")
    trap 'rm -f -- "$staging"' EXIT
    curl --fail --location --retry 3 --output "$staging" \
        'https://raw.pixls.us/getfile.php/2060/nice/Nikon%20-%20D70%20-%2012bit%2012bit%20compressed%20(Lossy%20(type%201))%20(3:2).NEF'
    printf '%s  %s\n' "$digest" "$staging" | sha256sum --check
    mv -- "$staging" "$filename"
fi
printf 'Fixture ready: %s\n' "$filename"
printf 'COMPOSITOR_TEST_NEF=%q cargo test --locked --lib raw::tests::real_nikon -- --ignored --nocapture\n' "$filename"
