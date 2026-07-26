#!/usr/bin/env bash
# Coverage note, since this is a test for a check that only fires on a machine
# configured differently from the one running it.
#
# The comparison is exercised in both directions through `--compare`, which
# takes two literal strings, so the matching case is covered without a second
# Xcode installed. The end-to-end paths are exercised against a stub
# `xcodebuild` on PATH, which also covers the extraction of the version line.
#
# What remains uncovered is exactly one thing: the real xcodebuild binary's
# output format. If Apple ever changes the first line of `xcodebuild -version`
# away from "Xcode <version>", this suite keeps passing and the check starts
# refusing on every machine. That is the safe direction to fail, and it is the
# same string CI compares, so CI would break identically and visibly.

set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/../.." && pwd)
preflight=$repo_root/scripts/preflight-toolchain.sh

bash -n "$preflight"

help=$("$preflight" --help)
grep -Fq -- "--compare" <<<"$help"
grep -Fq -- ".xcode-version" <<<"$help"

test_root=$(mktemp -d)
trap 'rm -rf "$test_root"' EXIT
output=$test_root/output.log

expect_success() {
  local label=$1
  shift
  if ! "$@" >"$output" 2>&1; then
    echo "$label: expected success, got exit $?" >&2
    cat "$output" >&2
    exit 1
  fi
}

expect_failure() {
  local label=$1
  local needle=$2
  shift 2
  if "$@" >"$output" 2>&1; then
    echo "$label: expected failure, but it succeeded" >&2
    cat "$output" >&2
    exit 1
  fi
  if ! grep -Fq "$needle" "$output"; then
    echo "$label: output did not mention '$needle'" >&2
    cat "$output" >&2
    exit 1
  fi
}

# --- the comparison itself, both directions ---------------------------------

expect_success "matching versions" \
  "$preflight" --compare "16.4" "Xcode 16.4"

expect_failure "mismatched versions" "Xcode 26.6" \
  "$preflight" --compare "16.4" "Xcode 26.6"

# The mismatch report has to name both sides; naming only one leaves the reader
# guessing which end to change.
"$preflight" --compare "16.4" "Xcode 26.6" >"$output" 2>&1 || true
grep -Fq "pinned by .xcode-version : Xcode 16.4" "$output"
grep -Fq "currently selected       : Xcode 26.6" "$output"
grep -Fq "xcode-select --switch" "$output"

# A near-miss is a mismatch. CI compares the whole string, so anything looser
# here could pass locally and still fail there.
expect_failure "patch-level near miss" "Xcode 16.4.1" \
  "$preflight" --compare "16.4" "Xcode 16.4.1"

# --- refusing rather than passing when there is nothing to compare ----------

expect_failure "empty pinned version" "malformed" \
  "$preflight" --compare "" "Xcode 16.4"

expect_failure "unparseable pinned version" "malformed" \
  "$preflight" --compare "not a version" "Xcode 16.4"

expect_failure "empty toolchain line" "expected a line like" \
  "$preflight" --compare "16.4" ""

expect_failure "unparseable toolchain line" "expected a line like" \
  "$preflight" --compare "16.4" "swift-driver version 1.90"

expect_failure "wrong argument count" "exactly two arguments" \
  "$preflight" --compare "16.4"

expect_failure "unknown option" "unknown option" \
  "$preflight" --nonsense

# --- end to end, against a stub xcodebuild ----------------------------------

fixture_repo=$test_root/repo
fixture_bin=$test_root/bin
mkdir -p "$fixture_repo/scripts" "$fixture_bin"
cp "$preflight" "$fixture_repo/scripts/preflight-toolchain.sh"
fixture_preflight=$fixture_repo/scripts/preflight-toolchain.sh

write_stub_xcodebuild() {
  cat >"$fixture_bin/xcodebuild" <<STUB
#!/usr/bin/env bash
echo "$1"
echo "Build version 17F113"
STUB
  chmod +x "$fixture_bin/xcodebuild"
}

printf '16.4\n' >"$fixture_repo/.xcode-version"
write_stub_xcodebuild "Xcode 16.4"
expect_success "end to end match" \
  env PATH="$fixture_bin:$PATH" "$fixture_preflight"

write_stub_xcodebuild "Xcode 26.6"
expect_failure "end to end mismatch" "not using the Xcode this repository pins" \
  env PATH="$fixture_bin:$PATH" "$fixture_preflight"

# Trailing whitespace in the pin file is tolerated; a version is still readable.
printf '  16.4  \n' >"$fixture_repo/.xcode-version"
write_stub_xcodebuild "Xcode 16.4"
expect_success "pin file with surrounding whitespace" \
  env PATH="$fixture_bin:$PATH" "$fixture_preflight"

# An empty pin file is not "no constraint", it is an unreadable constraint.
printf '\n' >"$fixture_repo/.xcode-version"
expect_failure "empty pin file" "is empty" \
  env PATH="$fixture_bin:$PATH" "$fixture_preflight"

rm -f "$fixture_repo/.xcode-version"
expect_failure "missing pin file" "no .xcode-version" \
  env PATH="$fixture_bin:$PATH" "$fixture_preflight"

# No xcodebuild at all must refuse, not pass for lack of evidence.
#
# Do not simplify this back to an empty directory. That was the first version,
# and it passed for the wrong reason: an empty PATH removes `bash` too, so the
# script died on its own `#!/usr/bin/env bash` line before reaching the check.
# `expect_failure` saw a non-zero exit and was satisfied. The assertion claimed
# "refuses when xcodebuild is missing" while actually proving "cannot start
# without a shell" -- a test verifying its own claim by accident.
#
# The fixture PATH therefore carries exactly the utilities the script itself
# uses, so the only thing absent is the thing under test. It was caught only
# because the error text did not match the expected message; a looser
# assertion would still be green and still be meaningless.
printf '16.4\n' >"$fixture_repo/.xcode-version"
without_xcodebuild=$test_root/without-xcodebuild
mkdir -p "$without_xcodebuild"
for tool in bash dirname tr head cat; do
  ln -sf "$(command -v "$tool")" "$without_xcodebuild/$tool"
done
expect_failure "xcodebuild absent" "not on PATH" \
  env PATH="$without_xcodebuild" "$fixture_preflight"

# A present but silent xcodebuild is the same problem wearing a different hat.
cat >"$fixture_bin/xcodebuild" <<'STUB'
#!/usr/bin/env bash
exit 0
STUB
chmod +x "$fixture_bin/xcodebuild"
expect_failure "xcodebuild produced no output" "produced no output" \
  env PATH="$fixture_bin:$PATH" "$fixture_preflight"

echo "preflight toolchain checks passed"
