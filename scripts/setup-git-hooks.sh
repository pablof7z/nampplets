#!/bin/sh
# Points this checkout's git hooks at the tracked scripts/git-hooks directory,
# so the AGENTS.md 600-line file-growth ratchet runs locally before CI does.
# Run this once per checkout: scripts/setup-git-hooks.sh
set -eu

repository_root=$(git rev-parse --show-toplevel)
cd "$repository_root"
git config core.hooksPath scripts/git-hooks
echo "core.hooksPath -> scripts/git-hooks"
