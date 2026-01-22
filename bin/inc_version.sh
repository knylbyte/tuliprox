#!/usr/bin/env bash
set -eo pipefail

WORKING_DIR=$(pwd)
BACKEND_DIR="${WORKING_DIR}/backend"

if ! command -v cargo-set-version &> /dev/null
then
    echo "🧨 cargo-set-version could not be found. Install it with 'cargo install cargo-edit'"
    exit 1
fi


# Read current version from Cargo.toml
OLD_VERSION=$(grep '^version' "${BACKEND_DIR}/Cargo.toml" | head -n1 | cut -d'"' -f2)

# Remove pre-release and build metadata (e.g., 1.0.0-dev → 1.0.0)
CLEAN_VERSION="${OLD_VERSION%%-*}"
CLEAN_VERSION="${CLEAN_VERSION%%+*}"
IFS='.' read -r major minor patch <<< "$CLEAN_VERSION"

case "$1" in
  k)
    ;;
  m) # Major bump
     ((major++))
     minor=0
     patch=0
     ;;
  p) # Minor bump
     ((minor++))
     patch=0
     ;;
  *) # Patch bump (default)
     ((patch++))
     ;;
esac

NEW_VERSION="${major}.${minor}.${patch}"


cargo set-version "$NEW_VERSION"

VERSION=v$NEW_VERSION
echo "🛠️ Set version $VERSION"
echo 