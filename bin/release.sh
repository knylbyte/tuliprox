#!/usr/bin/env bash
set -eo pipefail

WORKING_DIR=$(pwd)
RESOURCES_DIR="$WORKING_DIR/resources"
RELEASE_DIR="$WORKING_DIR/release"
FRONTEND_DIR="${WORKING_DIR}/frontend"
FRONTEND_BUILD_DIR="${FRONTEND_DIR}/dist"
BACKEND_DIR="${WORKING_DIR}/backend"

./bin/build_resources.sh

if ! command -v cargo-set-version &> /dev/null
then
    echo "🧨 cargo-set-version could not be found. Install it with 'cargo install cargo-edit'"
    exit 1
fi

cd "$FRONTEND_DIR" || (echo "🧨 Can't find frontend directory" && exit 1)

# Read current version from Cargo.toml
OLD_VERSION=$(grep '^version' "${BACKEND_DIR}/Cargo.toml" | head -n1 | cut -d'"' -f2)

IFS='.' read -r major minor patch <<< "$OLD_VERSION"

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

cd "$WORKING_DIR"

cargo set-version "$NEW_VERSION"

VERSION=v$NEW_VERSION
echo "🛠️ Building version $VERSION"

declare -A ARCHITECTURES=(
    [LINUX]=x86_64-unknown-linux-musl
    [WINDOWS]=x86_64-pc-windows-gnu
    [ARM7]=armv7-unknown-linux-musleabihf
    [AARCH64]=aarch64-unknown-linux-musl
    # [DARWIN]=x86_64-apple-darwin
)

declare -A DIRS=(
    [LINUX]=tuliprox_${VERSION}_linux_x86_64
    [WINDOWS]=tuliprox_${VERSION}_windows_x86_64
    [ARM7]=tuliprox_${VERSION}_armv7
    [AARCH64]=tuliprox_${VERSION}_aarch64_x86_64
    [DARWIN]=tuliprox_${VERSION}_apple-darwin_x86_64
)

# Special case mapping for binary extensions (e.g., Windows needs .exe)
declare -A BIN_EXTENSIONS=(
    [WINDOWS]=.exe
)

cd "$WORKING_DIR"
mkdir -p "$RELEASE_DIR"

# Clean previous builds
cargo clean || true

rm -rf "${FRONTEND_BUILD_DIR}"
cd "${FRONTEND_DIR}" && env RUSTFLAGS="--remap-path-prefix $HOME=~" trunk build --release
# Check if the frontend build directory exists
if [ ! -d "${FRONTEND_BUILD_DIR}" ]; then
    echo "🧨 Error: Web directory '${FRONTEND_BUILD_DIR}' does not exist."
    exit 1
fi

cd "$WORKING_DIR"

# Build binaries
for PLATFORM in "${!ARCHITECTURES[@]}"; do
    ARCHITECTURE=${ARCHITECTURES[$PLATFORM]}
    DIR=${DIRS[$PLATFORM]}
    ARC=${DIR}.tgz
    # Handle platform-specific binary file names
    if [[ -n "${BIN_EXTENSIONS[$PLATFORM]}" ]]; then
       BIN="${ARCHITECTURE}/release/tuliprox${BIN_EXTENSIONS[$PLATFORM]}"
    else
       BIN="${ARCHITECTURE}/release/tuliprox"
    fi

    rustup target add "$ARCHITECTURE"

    # Build for each platform
    cd "$WORKING_DIR"
    cargo clean || true # Clean before each build to avoid conflicts
    env RUSTFLAGS="--remap-path-prefix $HOME=~" cross build -p tuliprox --release --target "$ARCHITECTURE"

    # Create directories and copy binaries and config files
    cd target
    mkdir -p "$DIR"
    cp "$BIN" "$DIR"
    cp ../config/*.yml "$DIR"
    cp -rf "${FRONTEND_BUILD_DIR}" "$DIR"/web
    cp -rf "${RESOURCES_DIR}"/*.ts "$DIR"

    # Create archive for the platform
    if [[ $PLATFORM == "WINDOWS" ]]; then
        zip -r "$ARC" "$DIR"
    else
        tar cvzf "$ARC" "$DIR"
    fi

    CHECKSUM_FILE="checksum_${ARC}.txt"
    shasum -a 256 "$ARC" >> "$CHECKSUM_FILE"

    # Move the archive and checksum to the release folder
    RELEASE_PKG="$RELEASE_DIR/release_${VERSION}"
    mkdir -p "$RELEASE_PKG"
    mv "$CHECKSUM_FILE" "$ARC" "$RELEASE_PKG"
done

echo "🗑 Cleaning up build artifacts"
# Clean up the build directories
cd "$WORKING_DIR"
cargo clean

echo "📦 git commit version: ${VERSION}"
# Commit and tag release
git add .
git commit -m "release ${VERSION}"
git tag -a "$VERSION" -m "$VERSION"
git push
git push --tags
git push github
git push github --tags

echo "🎉 Done!"
