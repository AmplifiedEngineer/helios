#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
#
# Manual smoke check: runs `helios --smoke` against your real GitHub accounts
# and prints the Mine and Needs me counts. Never run it in CI.
#
# HELIOS_GH  the gh program to use (default: gh on PATH)
# HELIOS_BIN a built helios binary (default: cargo run)
set -eu

gh_program="${HELIOS_GH:-gh}"

if ! gh_path=$(command -v "$gh_program"); then
    echo "gh not found: $gh_program" >&2
    exit 1
fi

HELIOS_GH="$gh_path"
export HELIOS_GH

if [ -n "${HELIOS_BIN:-}" ]; then
    exec "$HELIOS_BIN" --smoke
fi

exec cargo run --quiet -- --smoke
