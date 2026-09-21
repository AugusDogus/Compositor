#!/usr/bin/env bash
# Photoshop-created ag-psd fixtures, downloaded separately so artwork stays out
# of this repository. CC 2019 (shapes), CS6 (adjustments), 22.5 (polygon), as
# identified by each source PSD's XMP creator metadata.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
destination=${COMPOSITOR_PSD_FIXTURE_DIR:-"$root/target/fixtures/psd-external"}
revision=387049670cb89b88fb8fe1b7c01aeacf98dd2e3b
mkdir -p -- "$destination"
staging=$(mktemp -d "$destination/.download-XXXXXX")
trap 'rm -rf -- "$staging"' EXIT

while read -r digest source filename; do
    if [[ -f "$destination/$filename" ]] && printf '%s  %s\n' "$digest" "$destination/$filename" | sha256sum --check --status; then
        continue
    fi
    curl --fail --location --retry 3 --output "$staging/$filename" \
        "https://raw.githubusercontent.com/Agamnentzar/ag-psd/$revision/$source"
    printf '%s  %s\n' "$digest" "$staging/$filename" | sha256sum --check
    mv -- "$staging/$filename" "$destination/$filename"
done <<'FIXTURES'
e6ce955cdcbf12e8dca78572734098404d3b8456dab32616ac2631fcb333f7b4 test/read-write/shapes/src.psd shapes.psd
60a8e5f4226345bc5adc8ea6a1ce2f65c36ff03000fdb94be98c8931a71c7bc2 test/read-write/adjustments/src.psd adjustments.psd
5454bec80651fc1ed6ec2e58524358bca1e56aa25091e20c8482faec1bdfbbe8 test/read/key-origin-shape-bbox/src.psd polygon.psd
FIXTURES

printf 'Fixtures ready in %s\nRun: cargo test --locked --test psd_external -- --ignored --test-threads=4\n' "$destination"
