variable "GHCR_NS" {
  default = "ghcr.io/euzu/tuliprox"
}

variable "ARCH_TAG" {
  default = "linux-amd64"
}

variable "PLATFORM" {
  default = "linux/amd64"
}

variable "VERSION" {
  default = "dev"
}

variable "CACHE_CONTEXT" {
  default = "."
}

variable "CACHE_DEST" {
  default = "/tmp/cache-out"
}

variable "DOCKER_IMAGE_DEST" {
  default = "/tmp/oci-artifacts"
}

variable "CARGO_HOME" {
  default = "/usr/local/cargo"
}

variable "SCCACHE_DIR" {
  default = "/var/cache/sccache"
}

variable "INLINE_CACHE" {
  default = "1"
}

target "common" {
  context    = "."
  dockerfile = "docker/ci.Dockerfile"

  args = {
    GHCR_NS               = "${GHCR_NS}"
    BUILDPLATFORM_TAG     = "${ARCH_TAG}"
    CARGO_HOME            = "${CARGO_HOME}"
    SCCACHE_DIR           = "${SCCACHE_DIR}"
    BUILDKIT_INLINE_CACHE = "${INLINE_CACHE}"
  }

  platforms = ["${PLATFORM}"]
}

target "cache-export" {
  inherits = ["common"]
  target   = "cache-export"

  contexts = {
    ctx_cache = "${CACHE_CONTEXT}"
  }

  cache_from = [
    "type=gha,scope=dev-cache-export-${ARCH_TAG}",
    "type=registry,ref=${GHCR_NS}:dev"
  ]

  cache_to = [
    "type=gha,scope=dev-cache-export-${ARCH_TAG},mode=max"
  ]

  output = [
    "type=local,dest=${CACHE_DEST}"
  ]
}

target "scratch-final" {
  inherits = ["common"]
  target   = "scratch-final"

  contexts = {
    ctx_cache = "${CACHE_CONTEXT}"
  }

  cache_from = [
    "type=gha,scope=dev-cache-export-${ARCH_TAG}",
    "type=registry,ref=${GHCR_NS}:dev"
  ]

  cache_to = [
    "type=gha,scope=dev-cache-export-${ARCH_TAG},mode=max"
  ]

  tags = [
    "${GHCR_NS}:dev-slim-${VERSION}-${ARCH_TAG}"
  ]

  output = [
    "type=docker,dest=${DOCKER_IMAGE_DEST}/scratch-final"
  ]
}

target "alpine-final" {
  inherits = ["common"]
  target   = "alpine-final"

  contexts = {
    ctx_cache = "${CACHE_CONTEXT}"
  }

  cache_from = [
    "type=gha,scope=dev-cache-export-${ARCH_TAG}",
    "type=registry,ref=${GHCR_NS}:dev"
  ]

  cache_to = [
    "type=gha,scope=dev-cache-export-${ARCH_TAG},mode=max"
  ]

  tags = [
    "${GHCR_NS}:dev-${VERSION}-${ARCH_TAG}"
  ]

  output = [
    "type=docker,dest=${DOCKER_IMAGE_DEST}/alpine-final"
  ]
}
