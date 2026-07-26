# Build from a clean checkout

The supported build uses Rust 1.89.0 from `rust-toolchain.toml` and Xcode 16.4
from `.xcode-version`. Xcode 16.4 supplies Apple Swift 6.1. The generated Swift
source is checked in so its API changes are reviewable; the reproducible
universal XCFramework is ignored and must never be committed.

## Prerequisites

- macOS with Xcode 16.4 installed at
  `/Applications/Xcode_16.4.app`;
- `rustup`;
- Git and Python 3;
- Node.js 22.14.0 for the trusted-shell gate.

## Exact clean-clone gate

Run these commands from outside any existing checkout:

```sh
git clone https://github.com/pablof7z/nampplets.git nampplets-clean
cd nampplets-clean

test ! -e target
test ! -e Packages/NMPNativeRuntime/.build
export DEVELOPER_DIR=/Applications/Xcode_16.4.app/Contents/Developer

rustup show active-toolchain
rustc --version
test "$(xcodebuild -version | head -n 1)" = "Xcode $(cat .xcode-version)"
swift --version
rustup target add aarch64-apple-darwin x86_64-apple-darwin

python3 -m unittest discover -s conformance/tests -p 'test_*.py'
python3 conformance/scripts/verify_baseline.py
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
node --test --test-timeout=10000 \
  web/trusted-shell/tests/trusted-shell.test.js \
  web/trusted-shell/tests/trusted-shell-apple-snapshot.test.js

scripts/tests/test-build-runtime-swift-xcframework.sh
scripts/build-runtime-swift-xcframework.sh --universal --check-bindings
git diff --exit-code -- \
  Cargo.lock \
  Packages/NMPNativeRuntime/Sources/NMPNativeRuntime/NMPNativeRuntime.swift

swift test --package-path Packages/NMPNativeRuntime --parallel
swift test --package-path platforms/apple --parallel
swift test \
  --package-path apps/workbench-macos/RuntimeWorkbenchPackage \
  --parallel
xcodebuild test \
  -workspace apps/workbench-macos/RuntimeWorkbench.xcworkspace \
  -scheme RuntimeWorkbench \
  -destination 'platform=macOS' \
  -derivedDataPath /tmp/nampplets-runtime-workbench-derived-data \
  -parallel-testing-enabled NO \
  -test-timeouts-enabled YES \
  -default-test-execution-time-allowance 60 \
  -maximum-test-execution-time-allowance 120
```

`--check-bindings` generates fresh UniFFI output and refuses to proceed if it
does not byte-match the checked-in Swift source. When an intentional Rust FFI
change makes that check fail, run the build script once without
`--check-bindings`, review and commit only
`Packages/NMPNativeRuntime/Sources/NMPNativeRuntime/NMPNativeRuntime.swift`,
then rerun the checked command. The
`Packages/NMPNativeRuntime/NMPNativeRuntime.xcframework` directory remains
ignored.

The universal build validates that its static library contains exactly
`arm64` and `x86_64` before creating the XCFramework. `--arm64-only` is a
faster local iteration mode, but it does not satisfy the clean-clone release
gate.

## How CI splits this sequence

The sequence above is the local gate and runs `--check-bindings` first because
one operator wants one verdict. CI deliberately does not: the `UniFFI Swift
bindings` job owns `--check-bindings` and nothing else, while `Apple package
and shared scheme` generates fresh bindings and then builds and tests Swift.
Neither job waits on the other, so a stale generated file and a broken source
tree are always reported as two independent failures on the same run. A stale
`NMPNativeRuntime.swift` fails the bindings job; it never prevents Swift from
compiling, and it never turns a compile error into a skipped step.
`scripts/ci/test_ci_workflow.py` enforces that topology.
