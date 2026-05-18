#!/usr/bin/env bash
set -euo pipefail
[[ $# -ne 1 ]] && { echo "Usage: $0 <new-version>"; exit 1; }
NEW_VERSION="$1"
[[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Error: semver format required"; exit 1; }
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CARGO_TOML="$REPO_ROOT/Cargo.toml"
CURRENT=$(grep -m1 '^version' "$CARGO_TOML" | sed 's/.*"\(.*\)".*/\1/')
echo "Current: $CURRENT -> New: $NEW_VERSION"
[[ "$CURRENT" == "$NEW_VERSION" ]] && { echo "Error: same version"; exit 1; }
sed -i "s/^version = \"$CURRENT\"/version = \"$NEW_VERSION\"/" "$CARGO_TOML"
cd "$REPO_ROOT" && cargo check --workspace 2>&1 | tail -1
git add Cargo.toml && git commit -m "chore: bump version to v$NEW_VERSION" && git tag "v$NEW_VERSION"
echo "Done! Run: git push && git push --tags"
