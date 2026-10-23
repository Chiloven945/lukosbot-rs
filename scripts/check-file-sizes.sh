#!/usr/bin/env bash
#
# Enforce the repository file-size rule:
#   > 500 lines: review and consider splitting
#   > 700 lines: split before merge (hard failure)
#
# Only hand-written, tracked source/documentation files are checked. Lock files,
# generated output, fixtures/data, and third-party code are excluded by not
# matching these extensions or by the pathspec exclusions below.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

warn_count=0
error_count=0

while IFS= read -r -d '' file; do
    lines="$(wc -l < "$file")"

    if (( lines > 700 )); then
        printf 'ERROR: %s has %d lines (hard limit: 700); split it by responsibility.\n' \
            "$file" "$lines" >&2
        ((error_count += 1))
    elif (( lines > 500 )); then
        printf 'WARN: %s has %d lines (review threshold: 500); consider splitting it.\n' \
            "$file" "$lines"
        ((warn_count += 1))
    fi
done < <(
    git ls-files -z --cached --others --exclude-standard -- \
        '*.rs' '*.java' '*.sh' '*.toml' '*.sql' '*.md' \
        ':!Cargo.lock' \
        ':!**/fixtures/**' \
        ':!**/snapshots/**'
)

if (( error_count > 0 )); then
    printf '\n%d file(s) exceed the hard 700-line limit.\n' "$error_count" >&2
    exit 1
fi

if (( warn_count > 0 )); then
    printf '\n%d file(s) exceed the 500-line review threshold.\n' "$warn_count"
else
    printf 'All checked files are within the 500-line review threshold.\n'
fi
