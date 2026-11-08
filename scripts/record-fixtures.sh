#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# Records `gh auth status --json hosts` into the recorded fixture. Fails if the
# output holds a token or not exactly one login, replaces the login and scopes
# with placeholders, and leaves the tracked fixture untouched on any failure.
# Review the file before committing it.
set -eu

out="$(dirname "$0")/../tests/fixtures/status/recorded-one-account.json"
tmp="$(mktemp)"
trap 'rm -f "$tmp" "$tmp.sed"' EXIT

gh auth status --json hosts >"$tmp"

# The prefixes are built from pieces so this file holds no token-shaped literal.
classic="gh[pousr]_"
fine="github""_pat_"
if grep -Eq '"token"[[:space:]]*:[[:space:]]*"[^"]' "$tmp" ||
    grep -Eq "$classic|$fine" "$tmp"; then
    echo "record-fixtures: output held a token, fixture left untouched" >&2
    exit 1
fi

if [ "$(grep -o '"login"' "$tmp" | wc -l | tr -d '[:space:]')" != 1 ]; then
    echo "record-fixtures: expected exactly one login, fixture left untouched" >&2
    exit 1
fi

placeholder_login="octocat"
placeholder_scopes="repo, read:org"
sed -e "s/\"login\":\"[^\"]*\"/\"login\":\"$placeholder_login\"/" \
    -e "s/\"scopes\":\"[^\"]*\"/\"scopes\":\"$placeholder_scopes\"/" \
    "$tmp" >"$tmp.sed"
mv "$tmp.sed" "$tmp"

mv "$tmp" "$out"
