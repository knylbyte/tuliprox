variable "ghcr_ns" {
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
    GHCR_NS             = var.ghcr_ns
    BUILDPLATFORM_TAG   = var.arch_tag
    CARGO_HOME          = var.cargo_home
    SCCACHE_DIR         = var.sccache_dir
    BUILDKIT_INLINE_CACHE = var.inline_cache
  }

  platforms = [var.platform]
}

target "cache-export" {
  inherits = ["common"]
  target   = "cache-export"

  contexts = {
    cache = "type=local,src=${var.cache_context}"
  }

  cache-from = [
    "type=gha,scope=dev-cache-export-${var.arch_tag}",
    "type=registry,ref=${var.ghcr_ns}:dev"
  ]

  cache-to = [
    "type=gha,scope=dev-cache-export-${var.arch_tag},mode=max"
  ]

  output = [
    "type=local,dest=${var.cache_output}"
  ]
}

target "scratch-final" {
  inherits = ["common"]
  target   = "scratch-final"

  contexts = {
    cache = "type=local,src=${var.cache_context}"
  }

  cache-from = [
    "type=gha,scope=dev-cache-export-${var.arch_tag}",
    "type=registry,ref=${var.ghcr_ns}:dev"
  ]

  cache-to = [
    "type=gha,scope=dev-cache-export-${var.arch_tag},mode=max"
  ]

  tags = [
    "${var.ghcr_ns}:dev-slim-${var.version}-${var.arch_tag}"
  ]

  push = true
}

target "alpine-final" {
  inherits = ["common"]
  target   = "alpine-final"

  contexts = {
    cache = "type=local,src=${var.cache_context}"
  }

  cache-from = [
    "type=gha,scope=dev-cache-export-${var.arch_tag}",
    "type=registry,ref=${var.ghcr_ns}:dev"
  ]

  cache-to = [
    "type=gha,scope=dev-cache-export-${var.arch_tag},mode=max"
  ]

  tags = [
    "${var.ghcr_ns}:dev-${var.version}-${var.arch_tag}"
  ]

  push = true
}
