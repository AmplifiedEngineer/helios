#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# Records `gh auth status --json hosts` into the recorded fixture, then fails
# if the output holds a non-empty token. Review the file before committing it.
set -eu

out="$(dirname "$0")/../tests/fixtures/status/recorded-one-account.json"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

gh auth status --json hosts >"$tmp"

if grep -Eq '"token"[[:space:]]*:[[:space:]]*"[^"]' "$tmp"; then
    echo "record-fixtures: output held a token, fixture left untouched" >&2
    exit 1
fi

mv "$tmp" "$out"
