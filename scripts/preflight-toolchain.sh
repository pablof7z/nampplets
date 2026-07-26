#!/usr/bin/env bash
# Refuse to proceed when the selected Xcode is not the one this repo pins.
#
# A local `swift build` or `swift test` uses whatever `xcode-select` points at.
# When that is not the pinned toolchain, a local green says nothing about the
# Apple CI job -- it can pass on a newer SDK while CI fails to compile, which
# is how a macOS 26 symbol reached main behind an `#available` guard and made
# every Apple job red.
#
# This is for humans. CI already pins the toolchain and asserts the same
# equality, so wiring this into the Apple job would add a check that can only
# ever pass there.

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/preflight-toolchain.sh [OPTION]

Compare the selected Xcode against the version pinned in .xcode-version and
exit non-zero when they disagree, when either is missing, or when either
cannot be read.

Options:
  --compare PINNED FIRST_LINE
                run the comparison against two literal strings instead of the
                machine's toolchain, where FIRST_LINE is the first line of
                `xcodebuild -version`. The test suite uses this so the
                comparison stays covered on a machine that has only one Xcode.
  -h, --help    show this help without checking anything
USAGE
}

fail() {
  echo "error: $1" >&2
  exit 1
}

# The whole point of the check, kept pure so it can be tested directly with
# both a matching and a mismatched pair.
#
# The equality is deliberately the same one CI runs, byte for byte
# (.github/workflows/ci.yml): `xcodebuild -version | head -n 1` against
# "Xcode <pinned>". Anything looser could pass here and still fail there,
# which would make this worse than no check at all.
toolchain_matches() {
  local pinned=$1
  local first_line=$2

  [[ -n "$pinned" ]] || return 2
  [[ -n "$first_line" ]] || return 3
  # A pinned value with whitespace or newlines is a malformed file, not a
  # version. Refuse rather than compare something meaningless.
  [[ "$pinned" =~ ^[0-9][0-9A-Za-z.]*$ ]] || return 2
  [[ "$first_line" == Xcode\ * ]] || return 3

  [[ "$first_line" == "Xcode $pinned" ]]
}

report_mismatch() {
  local pinned=$1
  local first_line=$2

  cat >&2 <<REPORT
error: this machine is not using the Xcode this repository pins.

  pinned by .xcode-version : Xcode $pinned
  currently selected       : $first_line

A local 'swift build' or 'swift test' uses the selected Xcode, so a green
result here does not predict the Apple CI job. It can compile against a newer
SDK than CI has and pass for that reason alone.

To fix, install Xcode $pinned and point at it:

  sudo xcode-select --switch /Applications/Xcode-$pinned.app

then re-run this script. Until the versions agree, treat any local Apple build
or test result as unverified against the pinned toolchain and say so when
reporting it.
REPORT
  exit 1
}

compare_mode=false
case "${1-}" in
  -h | --help)
    usage
    exit 0
    ;;
  --compare)
    compare_mode=true
    shift
    ;;
  "") ;;
  *)
    echo "error: unknown option: $1" >&2
    usage >&2
    exit 2
    ;;
esac

if [[ "$compare_mode" == true ]]; then
  [[ $# -eq 2 ]] || fail "--compare needs exactly two arguments: PINNED FIRST_LINE"
  pinned=$1
  first_line=$2
else
  script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
  repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
  pin_file=$repo_root/.xcode-version

  # Every branch below exits non-zero. A preflight that cannot read its inputs
  # must refuse, never quietly succeed -- "nothing to compare" reported as
  # success is the failure mode this exists to prevent.
  [[ -f "$pin_file" ]] || fail "no .xcode-version at $pin_file; cannot tell which Xcode this repository pins"
  pinned=$(tr -d '[:space:]' <"$pin_file")
  [[ -n "$pinned" ]] || fail ".xcode-version at $pin_file is empty; cannot tell which Xcode this repository pins"

  command -v xcodebuild >/dev/null 2>&1 \
    || fail "xcodebuild is not on PATH; cannot tell which Xcode is selected"

  # The only part not exercised by the test suite against a stub: the real
  # xcodebuild binary's output format.
  first_line=$(xcodebuild -version 2>/dev/null | head -n 1 || true)
  [[ -n "$first_line" ]] || fail "xcodebuild -version produced no output; cannot tell which Xcode is selected"
fi

status=0
toolchain_matches "$pinned" "$first_line" || status=$?

case "$status" in
  0)
    echo "Xcode $pinned matches the pinned toolchain."
    exit 0
    ;;
  2)
    fail "pinned version '$pinned' is not a version string; .xcode-version is malformed"
    ;;
  3)
    fail "could not read a version from '$first_line'; expected a line like 'Xcode 16.4'"
    ;;
  *)
    report_mismatch "$pinned" "$first_line"
    ;;
esac
