#!/usr/bin/env bash
set -eo pipefail

WORKING_DIR=$(pwd)
RESOURCES_DIR="$WORKING_DIR/resources"
RELEASE_DIR="$WORKING_DIR/release"
FRONTEND_DIR="${WORKING_DIR}/frontend"
FRONTEND_BUILD_DIR="${FRONTEND_DIR}/dist"
BACKEND_DIR="${WORKING_DIR}/backend"
BRANCH=$(git branch --show-current)
START_BRANCH="${BRANCH}"
START_HEAD="$(git rev-parse HEAD)"
RUN_KEY=""
DOCKER_BUILD_RUN_ID=""
ORIGIN_MASTER_AFTER_BUMP_SHA=""
ORIGIN_DEVELOP_BEFORE_SHA=""
ORIGIN_DEVELOP_AFTER_FF_SHA=""
GITHUB_MASTER_AFTER_BUMP_SHA=""
GITHUB_DEVELOP_BEFORE_SHA=""
GITHUB_DEVELOP_AFTER_FF_SHA=""

die() {
  echo "🧨 Error: $*" >&2
  exit 1
}

cleanup_on_failure() {
  exit_code=$?
  if [ "${exit_code}" -eq 0 ]; then
    return
  fi

  echo "🧹 Cleanup after failure (exit ${exit_code})" >&2

  if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    run_id="${DOCKER_BUILD_RUN_ID:-}"
    if [ -z "${run_id}" ] && [ -n "${RUN_KEY:-}" ]; then
      run_id="$(
        gh run list \
          -w docker-build.yml \
          -e workflow_dispatch \
          -b master \
          -L 20 \
          --json databaseId,displayTitle \
          --jq ".[] | select(.displayTitle | contains(\"${RUN_KEY}\")) | .databaseId" \
          | head -n 1
      )"
    fi

    if [ -n "${run_id}" ]; then
      status="$(gh run view "${run_id}" --json status --jq .status 2>/dev/null || true)"
      if [ -n "${status}" ] && [ "${status}" != "completed" ]; then
        echo "🛑 Cancelling docker-build workflow run ${run_id}" >&2
        gh run cancel "${run_id}" >/dev/null 2>&1 || true
      fi
    fi
  fi

  echo "↩️ Resetting local git state to ${START_BRANCH}@${START_HEAD}" >&2
  git reset --hard "${START_HEAD}" >/dev/null 2>&1 || true
  git checkout -f "${START_BRANCH}" >/dev/null 2>&1 || true

  if [ -n "${ORIGIN_MASTER_AFTER_BUMP_SHA}" ]; then
    echo "⏪ Reverting remote 'origin/master' (force-with-lease)" >&2
    git push --force-with-lease=refs/heads/master:"${ORIGIN_MASTER_AFTER_BUMP_SHA}" origin "${START_HEAD}:refs/heads/master" >/dev/null 2>&1 || true
  fi

  if [ -n "${ORIGIN_DEVELOP_AFTER_FF_SHA}" ] && [ -n "${ORIGIN_DEVELOP_BEFORE_SHA}" ]; then
    echo "⏪ Reverting remote 'origin/develop' (force-with-lease)" >&2
    git push --force-with-lease=refs/heads/develop:"${ORIGIN_DEVELOP_AFTER_FF_SHA}" origin "${ORIGIN_DEVELOP_BEFORE_SHA}:refs/heads/develop" >/dev/null 2>&1 || true
  fi

  if [ -n "${GITHUB_MASTER_AFTER_BUMP_SHA}" ]; then
    echo "⏪ Reverting remote 'github/master' (force-with-lease)" >&2
    git push --force-with-lease=refs/heads/master:"${GITHUB_MASTER_AFTER_BUMP_SHA}" github "${START_HEAD}:refs/heads/master" >/dev/null 2>&1 || true
  fi

  if [ -n "${GITHUB_DEVELOP_AFTER_FF_SHA}" ] && [ -n "${GITHUB_DEVELOP_BEFORE_SHA}" ]; then
    echo "⏪ Reverting remote 'github/develop' (force-with-lease)" >&2
    git push --force-with-lease=refs/heads/develop:"${GITHUB_DEVELOP_AFTER_FF_SHA}" github "${GITHUB_DEVELOP_BEFORE_SHA}:refs/heads/develop" >/dev/null 2>&1 || true
  fi
}

trap cleanup_on_failure EXIT INT TERM

# Validate release strategy
if [ $# -ne 1 ]; then
  die "Release strategy required (major|minor)"
fi

if [ "$BRANCH" != "master" ]; then
  die "Creating the release from your current branch '${BRANCH}' is prohibited!"
fi

# Guards: ensure we're releasing the right state
if ! git diff --quiet || ! git diff --cached --quiet; then
  die "Working tree is not clean. Commit/stash your changes first."
fi

git fetch --quiet --tags origin master develop || die "Failed to fetch from 'origin'."

if [ "$(git rev-parse HEAD)" != "$(git rev-parse origin/master)" ]; then
  die "Local 'master' is not at 'origin/master'. Please pull/push before releasing."
fi

if ! git merge-base --is-ancestor origin/develop HEAD; then
  die "'master' does not contain 'origin/develop'. Merge develop into master before releasing."
fi

ORIGIN_DEVELOP_BEFORE_SHA="$(git rev-parse origin/develop)"
if git remote get-url github >/dev/null 2>&1; then
  git fetch --quiet github develop || die "Failed to fetch from 'github'."
  GITHUB_DEVELOP_BEFORE_SHA="$(git rev-parse github/develop)"
fi

LAST_TAG="$(git describe --tags --abbrev=0 2>/dev/null || true)"
if [ -n "$LAST_TAG" ]; then
  if git diff --quiet "${LAST_TAG}..HEAD" -- backend frontend shared resources docker config Cargo.toml Cargo.lock; then
    die "No relevant changes since last release tag '${LAST_TAG}'."
  fi
fi

if ! command -v gh &> /dev/null; then
  die "GitHub CLI could not be found. Please install gh toolset: https://cli.github.com"
fi

case "$1" in
  major) ./bin/inc_version.sh m ;;
  minor) ./bin/inc_version.sh p ;;
  *) die "Unknown option '$1' (expected: major|minor)" ;;
esac

# Marker: version bump push + trigger docker-build workflow (master)
echo "📦 Committing version bump"
FILES=(Cargo.lock backend/Cargo.lock backend/Cargo.toml frontend/Cargo.toml shared/Cargo.toml)
for f in "${FILES[@]}"; do
  if [ -f "$f" ]; then
    git add "$f"
  fi
done

if git diff --cached --quiet; then
  die "Version bump produced no changes to commit."
fi

BUMP_VERSION="$(grep '^version' "${BACKEND_DIR}/Cargo.toml" | head -n1 | cut -d'"' -f2)"
if [ -z "${BUMP_VERSION}" ]; then
  die "Failed to read version from '${BACKEND_DIR}/Cargo.toml' after bump."
fi

gh auth status >/dev/null 2>&1 || die "Not logged into GitHub CLI. Run 'gh auth login' first."

RUN_KEY="$(uuidgen 2>/dev/null || true)"
if [ -z "${RUN_KEY}" ]; then
  RUN_KEY="run-$(date +%s)-$$"
fi

RUN_KEY="${1}-${RUN_KEY}"

# Read current version from Cargo.toml
VERSION=$(grep '^version' "${BACKEND_DIR}/Cargo.toml" | head -n1 | cut -d'"' -f2)

read -rp "Releasing version: '${VERSION}', please confirm? [y/N] " answer

# Default 'N', cancel, if not 'y' or 'Y'
if [[ ! "$answer" =~ ^[Yy]$ ]]; then
    die "Canceled."
fi

git commit -m "ci: bump version v${BUMP_VERSION}"
git push origin HEAD:master
ORIGIN_MASTER_AFTER_BUMP_SHA="$(git rev-parse HEAD)"
if git remote get-url github >/dev/null 2>&1; then
  # git push github HEAD:master
  GITHUB_MASTER_AFTER_BUMP_SHA="${ORIGIN_MASTER_AFTER_BUMP_SHA}"
fi

echo "🚀 Triggering docker-build workflow (master, key: ${RUN_KEY})"
gh workflow run docker-build.yml --ref master -f branch=master -f choice=none -f run_key="${RUN_KEY}"

echo "🔎 Resolving docker-build workflow run id..."
for _ in {1..30}; do
  DOCKER_BUILD_RUN_ID="$(
    gh run list \
      -w docker-build.yml \
      -e workflow_dispatch \
      -b master \
      -L 20 \
      --json databaseId,displayTitle \
      --jq ".[] | select(.displayTitle | contains(\"${RUN_KEY}\")) | .databaseId" \
      | head -n 1
  )"
  if [ -n "${DOCKER_BUILD_RUN_ID}" ]; then
    break
  fi
  sleep 2
done

if [ -z "${DOCKER_BUILD_RUN_ID}" ]; then
  die "Timed out waiting for docker-build workflow run id (run_key=${RUN_KEY})."
fi

echo "🧩 docker-build run id: ${DOCKER_BUILD_RUN_ID}"

./bin/build_resources.sh

cd "$FRONTEND_DIR" || die "Can't find frontend directory"

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
    die "🧨 Error: Web directory '${FRONTEND_BUILD_DIR}' does not exist."
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

# Marker: wait for docker-build workflow completion
echo "⏳ Waiting for docker-build workflow to finish (run id: ${DOCKER_BUILD_RUN_ID})"
gh run watch "${DOCKER_BUILD_RUN_ID}" --exit-status

echo "🔀 Back-merging master into develop (fast-forward)"
git fetch --quiet origin develop || die "Failed to fetch 'origin/develop'."
if ! git merge-base --is-ancestor origin/develop HEAD; then
  die "Refusing to update develop: 'origin/develop' is not an ancestor of current master."
fi

git push origin HEAD:develop
ORIGIN_DEVELOP_AFTER_FF_SHA="$(git rev-parse HEAD)"
if git remote get-url github >/dev/null 2>&1; then
  # git push github HEAD:develop
  GITHUB_DEVELOP_AFTER_FF_SHA="${ORIGIN_DEVELOP_AFTER_FF_SHA}"
fi

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
if git remote get-url github >/dev/null 2>&1; then
  # git push github
  # git push github --tags
fi

echo "🎉 Done!"
