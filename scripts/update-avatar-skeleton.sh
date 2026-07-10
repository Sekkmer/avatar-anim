#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT
readonly DESTINATION="${ROOT}/assets/avatar_skeleton.xml"
readonly SOURCE_URL="${AVATAR_SKELETON_URL:-https://raw.githubusercontent.com/secondlife/viewer/develop/indra/newview/character/avatar_skeleton.xml}"

temporary="$(mktemp)"
trap 'rm -f -- "${temporary}"' EXIT

curl --fail --location --silent --show-error "${SOURCE_URL}" --output "${temporary}"

if ! grep -q '<linden_skeleton ' "${temporary}"; then
    echo "Downloaded file is not a Linden skeleton: ${SOURCE_URL}" >&2
    exit 1
fi

if cmp --silent "${temporary}" "${DESTINATION}"; then
    echo "avatar_skeleton.xml is already current"
    exit 0
fi

install -m 0644 "${temporary}" "${DESTINATION}"
cargo test --manifest-path "${ROOT}/Cargo.toml" embedded_avatar_skeleton_is_available
echo "Updated assets/avatar_skeleton.xml from ${SOURCE_URL}"
