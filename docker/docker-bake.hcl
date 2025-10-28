variable "arg-ghcr_ns" {
  default = "ghcr.io/example/repo"
}

variable "arg-arch_tag" {
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

variable "arg-cargo_home" {
  default = "/usr/local/cargo"
}

variable "arg-sccache_dir" {
  default = "/var/cache/sccache"
}

variable "arg-inline_cache" {
  default = "1"
}

target "common" {
  context    = "."
  dockerfile = "docker/ci.Dockerfile"

  args = {
    GHCR_NS               = "${arg-ghcr_ns}"
    BUILDPLATFORM_TAG     = "${arg-arch_tag}"
    CARGO_HOME            = "${arg-cargo_home}"
    SCCACHE_DIR           = "${arg-sccache_dir}"
    BUILDKIT_INLINE_CACHE = "${arg-inline_cache}"
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
    "type=registry,ref=${ghcr_ns}:dev"
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
    "type=registry,ref=${ghcr_ns}:dev"
  ]

  cache_to = [
    "type=gha,scope=dev-cache-export-${arch_tag},mode=max"
  ]

  tags = [
    "${ghcr_ns}:dev-slim-${version}-${arch_tag}"
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
    "type=registry,ref=${ghcr_ns}:dev"
  ]

  cache_to = [
    "type=gha,scope=dev-cache-export-${arch_tag},mode=max"
  ]

  tags = [
    "${ghcr_ns}:dev-${version}-${arch_tag}"
  ]

  push = true
}
