variable "ghcr_namespace" {
  default = "ghcr.io/example/repo"
}

variable "arch_tag" {
  default = "linux-amd64"
}

variable "platform" {
  default = "linux/amd64"
}

variable "version" {
  default = "dev"
}

variable "cache_context" {
  default = "."
}

variable "cache_output" {
  default = "/tmp/cache-out"
}

variable "cargo_home" {
  default = "/usr/local/cargo"
}

variable "sccache_dir" {
  default = "/var/cache/sccache"
}

variable "inline_cache" {
  default = "1"
}

target "common" {
  context    = "."
  dockerfile = "docker/ci.Dockerfile"

  args = {
    GHCR_NS               = "${ghcr_namespace}"
    BUILDPLATFORM_TAG     = "${arch_tag}"
    CARGO_HOME            = "${cargo_home}"
    SCCACHE_DIR           = "${sccache_dir}"
    BUILDKIT_INLINE_CACHE = "${inline_cache}"
  }

  platforms = ["${platform}"]
}

target "cache-export" {
  inherits = ["common"]
  target   = "cache-export"

  contexts = {
    cache = "type=local,src=${cache_context}"
  }

  cache_from = [
    "type=gha,scope=dev-cache-export-${arch_tag}",
    "type=registry,ref=${ghcr_namespace}:dev"
  ]

  cache_to = [
    "type=gha,scope=dev-cache-export-${arch_tag},mode=max"
  ]

  output = [
    "type=local,dest=${cache_output}"
  ]
}

target "scratch-final" {
  inherits = ["common"]
  target   = "scratch-final"

  contexts = {
    cache = "type=local,src=${cache_context}"
  }

  cache_from = [
    "type=gha,scope=dev-cache-export-${arch_tag}",
    "type=registry,ref=${ghcr_namespace}:dev"
  ]

  cache_to = [
    "type=gha,scope=dev-cache-export-${arch_tag},mode=max"
  ]

  tags = [
    "${ghcr_namespace}:dev-slim-${version}-${arch_tag}"
  ]

  push = true
}

target "alpine-final" {
  inherits = ["common"]
  target   = "alpine-final"

  contexts = {
    cache = "type=local,src=${cache_context}"
  }

  cache_from = [
    "type=gha,scope=dev-cache-export-${arch_tag}",
    "type=registry,ref=${ghcr_namespace}:dev"
  ]

  cache_to = [
    "type=gha,scope=dev-cache-export-${arch_tag},mode=max"
  ]

  tags = [
    "${ghcr_namespace}:dev-${version}-${arch_tag}"
  ]

  push = true
}
