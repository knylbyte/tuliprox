#!/bin/bash
set -euo pipefail

# Check for required environment variable
if [ ! -f "${HOME}/.ghcr.io" ]; then
    echo "🧨 Error: ${HOME}/.ghcr.io file not found"
    exit 1
fi
source "${HOME}/.ghcr.io"

# Function to print usage instructions
print_usage() {
    echo "Usage: $(basename "$0") <branch>"
    echo
    echo "Arguments:"
    echo "  branch    Git branch name (only 'master' and 'develop' are supported)"
    echo
    echo "Examples:"
    echo "  $(basename "$0") master    # Builds and pushes with :latest tag"
    echo "  $(basename "$0") develop   # Builds and pushes with :dev tag"
    exit 1
}

# Validate arguments
if [ $# -ne 1 ]; then
    echo "🧨 Error: Exactly one argument required"
    print_usage
fi

BRANCH="$1"

# Validate branch
case "$BRANCH" in
    master)
        TAG_SUFFIX="latest"
        ;;
    develop)
        TAG_SUFFIX="dev"
        ;;
    *)
        echo "🧨 Error: Branch '$BRANCH' is not supported. Only 'master' and 'develop' are allowed."
        exit 1
        ;;
esac

echo "🚀 Building Docker images for branch: $BRANCH (tag: $TAG_SUFFIX)"

# Set up directories
WORKING_DIR=$(pwd)
BIN_DIR="${WORKING_DIR}/bin"
RESOURCES_DIR="${WORKING_DIR}/resources"
DOCKER_DIR="${WORKING_DIR}/docker"
BACKEND_DIR="${WORKING_DIR}/backend"
FRONTEND_DIR="${WORKING_DIR}/frontend"
FRONTEND_BUILD_DIR="${FRONTEND_DIR}/dist"

# Define architectures and their corresponding builds
declare -A ARCHITECTURES=(
    [LINUX]=x86_64-unknown-linux-musl
    [AARCH64]=aarch64-unknown-linux-musl
)

declare -A BUILDS=(
    [LINUX]="tuliprox:scratch-final tuliprox-alpine:alpine-final"
    [AARCH64]="tuliprox-aarch64:scratch-final"
)

# Get version from Cargo.toml
VERSION=$(grep -Po '^version\s*=\s*"\K[0-9\.]+' "${BACKEND_DIR}/Cargo.toml")
if [ -z "${VERSION}" ]; then
    echo "🧨 Error: Failed to determine the version from Cargo.toml"
    exit 1
fi

echo "📦 Version: ${VERSION}"

# Build resources if needed
if [ ! -f "${BIN_DIR}/build_resources.sh" ]; then
    echo "🛠️ Building resources..."
    "${BIN_DIR}/build_resources.sh"
fi

# Build frontend
echo "🎨 Building frontend..."
rm -rf "${FRONTEND_BUILD_DIR}"
cd "${FRONTEND_DIR}" && env RUSTFLAGS="--remap-path-prefix $HOME=~" trunk build --release

# Check if the frontend build directory exists
if [ ! -d "${FRONTEND_BUILD_DIR}" ]; then
    echo "🧨 Error: Frontend build directory '${FRONTEND_BUILD_DIR}' does not exist"
    exit 1
fi

cd "$WORKING_DIR"

declare -a BUILT_IMAGES=()

# Build for each platform
for PLATFORM in "${!ARCHITECTURES[@]}"; do
    ARCHITECTURE=${ARCHITECTURES[$PLATFORM]}
    
    echo "🏗️ Building binary for architecture: $ARCHITECTURE"
    
    cargo clean || true
    env RUSTFLAGS="--remap-path-prefix $HOME=~" cross build -p tuliprox --release --target "$ARCHITECTURE"
    
    BINARY_PATH="${WORKING_DIR}/target/${ARCHITECTURE}/release/tuliprox"
    if [ ! -f "$BINARY_PATH" ]; then
        echo "🧨 Error: Binary $BINARY_PATH does not exist"
        exit 1
    fi
    
    # Prepare Docker context
    echo "📋 Preparing Docker context for $PLATFORM..."
    cp "$BINARY_PATH" "${DOCKER_DIR}/"
    rm -rf "${DOCKER_DIR}/web"
    cp -r "${FRONTEND_BUILD_DIR}" "${DOCKER_DIR}/web"
    cp -r "${RESOURCES_DIR}" "${DOCKER_DIR}/resources"
    
    cd "${DOCKER_DIR}"
    echo "🐳 Building Docker images for platform: $PLATFORM"
    
    # Build all configured images for this platform
    for pair in ${BUILDS[$PLATFORM]}; do
        IMAGE_NAME="${pair%%:*}"
        BUILD_TARGET="${pair##*:}"
        
        echo "🎯 Building ${IMAGE_NAME} with target ${BUILD_TARGET}"
        
        # Determine Docker platform from Rust target
        case "$ARCHITECTURE" in
            aarch64-*)
                DOCKER_PLATFORM="linux/arm64"
                ;;
            x86_64-*)
                DOCKER_PLATFORM="linux/amd64"
                ;;
            armv7-*)
                DOCKER_PLATFORM="linux/arm/v7"
                ;;
            *)
                DOCKER_PLATFORM="linux/amd64"  # Default fallback
                ;;
        esac
        
        echo "🏗️ Building for platform: $DOCKER_PLATFORM"
        
        # Build with version tag using buildx for multi-platform support
        docker buildx build -f Dockerfile-manual \
            -t "ghcr.io/euzu/${IMAGE_NAME}:${VERSION}" \
            --target "$BUILD_TARGET" \
            --platform "$DOCKER_PLATFORM" \
            --load \
            .
        
        # Tag with branch-specific tag
        docker tag "ghcr.io/euzu/${IMAGE_NAME}:${VERSION}" "ghcr.io/euzu/${IMAGE_NAME}:${TAG_SUFFIX}"
        
        BUILT_IMAGES+=("ghcr.io/euzu/${IMAGE_NAME}:${VERSION}")
        BUILT_IMAGES+=("ghcr.io/euzu/${IMAGE_NAME}:${TAG_SUFFIX}")
    done
    
    # Clean up Docker context for next iteration
    rm -f "${DOCKER_DIR}/tuliprox"
done

cd "$WORKING_DIR"

# Login and push all images
echo "🔑 Logging into GitHub Container Registry..."
docker login ghcr.io -u euzu -p "${GHCR_IO_TOKEN}"

for img in "${BUILT_IMAGES[@]}"; do
    echo "📤 Pushing $img"
    docker push "$img"
done

# Final cleanup
echo "🗑️ Cleaning up Docker context..."
rm -rf "${DOCKER_DIR}/web"
rm -f "${DOCKER_DIR}/tuliprox"
rm -rf "${DOCKER_DIR}/resources"

echo "🎉 Docker images for branch '$BRANCH' (version ${VERSION}) have been successfully built and pushed!"
echo "📋 Built images:"
for img in "${BUILT_IMAGES[@]}"; do
    echo "   - $img"
done
