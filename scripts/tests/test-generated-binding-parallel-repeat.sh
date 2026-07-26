#!/usr/bin/env bash
# Repeatedly exercise the real generated UniFFI callbacks under SwiftPM workers.

set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/../.." && pwd)
repeat_runs=${GENERATED_BINDING_REPEAT_RUNS:-12}

case "$repeat_runs" in
  ''|*[!0-9]*)
    echo "error: GENERATED_BINDING_REPEAT_RUNS must be an integer" >&2
    exit 2
    ;;
esac
if (( repeat_runs < 1 || repeat_runs > 32 )); then
  echo "error: GENERATED_BINDING_REPEAT_RUNS must be between 1 and 32" >&2
  exit 2
fi

for ((run_index = 1; run_index <= repeat_runs; run_index += 1)); do
  echo "== generated binding parallel run $run_index/$repeat_runs =="
  swift test \
    --package-path "$repo_root/Packages/NMPNativeRuntime" \
    --parallel \
    --filter NMPNativeRuntimeTests.GeneratedBindingTests
done

echo "generated binding parallel repeat: $repeat_runs/$repeat_runs passed"
