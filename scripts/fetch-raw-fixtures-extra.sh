#!/usr/bin/env bash
# Public-domain CC0 sensor fixtures from raw.pixls.us.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
destination="$root/target/fixtures/raw-external"
mkdir -p -- "$destination"
fetch() {
    local name=$1 digest=$2 url=$3 staging
    if [[ -f "$destination/$name" ]] && printf '%s  %s\n' "$digest" "$destination/$name" | sha256sum --check --status; then
        return
    fi
    staging=$(mktemp "$destination/.download-XXXXXX")
    if ! curl --fail --location --retry 3 --output "$staging" "$url" || ! printf '%s  %s\n' "$digest" "$staging" | sha256sum --check; then
        rm -f -- "$staging"
        return 1
    fi
    mv -- "$staging" "$destination/$name"
}
fetch fujifilm-xpro1.raf 0b1046c299d9eb222a8f003bd047f2a34f507da0d1704ac32cefbc82f799d121 \
    'https://raw.pixls.us/getfile.php/1191/nice/Fujifilm%20-%20X-Pro1%20-%2012bit%2012bit%20uncompressed%20(3:2).RAF'
printf 'Fixtures ready. Run cargo test --lib raw::libraw::tests::real_xtrans -- --ignored\n'
