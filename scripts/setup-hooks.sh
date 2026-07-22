#!/usr/bin/env sh
# Configure this clone to use the repository-owned hooks. Safe to run repeatedly.
set -eu

git rev-parse --is-inside-work-tree >/dev/null
git config core.hooksPath .githooks
printf '%s\n' 'Installed repository hooks from .githooks.'
