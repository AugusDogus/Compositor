#!/usr/bin/env bash
# Combine validated architecture feeds. Does not contact GitHub or publish files.
set -euo pipefail
[[ $# -ge 3 ]] || { printf 'Usage: %s output.json architecture-feed.json architecture-feed.json [...]\n' "$0" >&2; exit 1; }
output="$1"
shift
stage="$(mktemp "${output}.XXXXXX")"
trap 'rm -f -- "$stage"' EXIT
jq -e -s '
    if any(.[]; (.version | type) != "string" or (.pub_date | type) != "string" or (.platforms | type) != "object") then
        error("Each update feed needs version, pub_date, and platforms")
    elif ([.[].version] | unique | length) != 1 then
        error("Architecture update feeds have different versions")
    else
        reduce .[] as $feed ({version:.[0].version, pub_date:([.[].pub_date] | max), notes:.[0].notes, platforms:{}};
            .platforms as $seen |
            if any($feed.platforms | keys[]; . as $key | $seen | has($key)) then
                error("Architecture update feeds repeat a platform")
            else .platforms += $feed.platforms end
        ) |
        if (.platforms | keys) != ["linux-aarch64", "linux-x86_64"] then
            error("A release must include both Linux x86_64 and aarch64 executables")
        else
            . as $merged |
            if any(.platforms | to_entries[]; .value.url !=
                ("https://github.com/AugusDogus/Compositor/releases/download/v" + $merged.version + "/Compositor-" + $merged.version + "-" + .key + ".bin")) then
                error("An update executable URL does not match its release version and architecture")
            else . end
        end
    end
' "$@" > "$stage"
mv -f -- "$stage" "$output"
