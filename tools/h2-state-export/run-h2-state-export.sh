#!/usr/bin/env bash
#
# Run the Java 25 compact-source H2 state exporter.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SOURCE="$SCRIPT_DIR/H2StateExport.java"

H2_JAR="${H2_JAR:-}"
H2_URL="${H2_URL:-jdbc:h2:file:./data/lukosbot2}"
H2_OUTPUT="${H2_OUTPUT:-./bot-state.jsonl}"
H2_USER="${H2_USER:-sa}"
H2_PASSWORD="${H2_PASSWORD-}"
JAVA_BIN="${JAVA_BIN:-java}"

usage() {
    cat <<EOF
Usage: $(basename -- "$0") [options]

Run the Java 25 compact-source H2StateExport.java exporter.

Options:
  --jar PATH       H2 JDBC jar
  --url JDBC_URL   H2 JDBC URL
  --output PATH    Output JSONL path
  --user NAME      H2 user (default: sa)
  --password PASS  H2 password (default: empty)
  -h, --help       Show this help

Environment:
  H2_JAR, H2_URL, H2_OUTPUT, H2_USER, H2_PASSWORD, JAVA_BIN

Requires JDK 25 or newer because H2StateExport.java is a compact source file.
EOF
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

latest_h2_jar_in() {
    local root="$1"
    [[ -d "$root" ]] || return 0
    find "$root" -type f -name 'h2-*.jar' \
        ! -name '*-sources.jar' \
        ! -name '*-javadoc.jar' \
        2>/dev/null | sort -V | tail -n 1 || true
}

locate_h2_jar() {
    local candidate

    for candidate in \
        "$SCRIPT_DIR/h2.jar" \
        "$PWD/h2.jar" \
        "$SCRIPT_DIR/../h2.jar"
    do
        if [[ -f "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done

    if [[ -n "${HOME:-}" ]]; then
        candidate="$(latest_h2_jar_in "$HOME/.m2/repository/com/h2database/h2")"
        if [[ -n "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi

        candidate="$(latest_h2_jar_in "$HOME/.gradle/caches/modules-2/files-2.1/com.h2database/h2")"
        if [[ -n "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    fi

    return 1
}

while (( $# > 0 )); do
    case "$1" in
        --jar)
            (( $# >= 2 )) || die "--jar requires a path"
            H2_JAR="$2"
            shift 2
            ;;
        --url)
            (( $# >= 2 )) || die "--url requires a JDBC URL"
            H2_URL="$2"
            shift 2
            ;;
        --output)
            (( $# >= 2 )) || die "--output requires a path"
            H2_OUTPUT="$2"
            shift 2
            ;;
        --user)
            (( $# >= 2 )) || die "--user requires a name"
            H2_USER="$2"
            shift 2
            ;;
        --password)
            (( $# >= 2 )) || die "--password requires a value"
            H2_PASSWORD="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown option: $1"
            ;;
    esac
done

command -v "$JAVA_BIN" >/dev/null 2>&1 || die "$JAVA_BIN was not found"
[[ -f "$SOURCE" ]] || die "exporter source was not found: $SOURCE"

JAVA_VERSION_OUTPUT="$("$JAVA_BIN" -version 2>&1 || true)"
JAVA_VERSION="$(printf '%s\n' "$JAVA_VERSION_OUTPUT" \
    | awk -F'"' '/version/ {print $2; exit}')"
[[ -n "$JAVA_VERSION" ]] || die "could not determine the Java version"
JAVA_MAJOR="${JAVA_VERSION%%.*}"
JAVA_MAJOR="${JAVA_MAJOR%%-*}"
[[ "$JAVA_MAJOR" =~ ^[0-9]+$ ]] || die "could not parse the Java major version: $JAVA_VERSION"
(( JAVA_MAJOR >= 25 )) \
    || die "H2StateExport.java requires JDK 25 or newer (found $JAVA_VERSION)"

if [[ -z "$H2_JAR" ]]; then
    H2_JAR="$(locate_h2_jar || true)"
fi
[[ -n "$H2_JAR" ]] || die "H2 jar was not found; set H2_JAR=/path/to/h2.jar"
[[ -f "$H2_JAR" ]] || die "H2 jar does not exist: $H2_JAR"
[[ -n "$H2_URL" ]] || die "H2 URL must not be empty"
[[ -n "$H2_OUTPUT" ]] || die "output path must not be empty"

OUTPUT_DIR="$(dirname -- "$H2_OUTPUT")"
mkdir -p -- "$OUTPUT_DIR"

printf 'H2 state export\n'
printf '  source : %s\n' "$SOURCE"
printf '  h2 jar : %s\n' "$H2_JAR"
printf '  url    : %s\n' "$H2_URL"
printf '  output : %s\n' "$H2_OUTPUT"
printf '  user   : %s\n' "$H2_USER"

"$JAVA_BIN" -cp "$H2_JAR" "$SOURCE" \
    "$H2_URL" "$H2_OUTPUT" "$H2_USER" "$H2_PASSWORD" \
    || die "H2 state export failed"
