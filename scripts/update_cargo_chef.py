#!/usr/bin/env python3
"""Generate cargo-chef manifests and lockfiles for backend/frontend workspaces."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Set, Tuple

try:  # Python 3.11+
    import tomllib  # type: ignore[attr-defined]
except ModuleNotFoundError:  # pragma: no cover - fallback for older interpreters
    import tomli as tomllib  # type: ignore[no-redef]

ROOT = Path(__file__).resolve().parent.parent
CHEF_DIR = ROOT / "docker" / "build-tools" / "cargo-chef"
CHECKSUM_PATH = CHEF_DIR / "checksums.json"
@dataclass
class PackageChange:
    name: str
    source: str
    change_type: str  # "added" | "removed" | "updated"
    old: str | None = None
    new: str | None = None


@dataclass
class TargetSummary:
    target: str
    added: List[PackageChange]
    removed: List[PackageChange]
    updated: List[PackageChange]

    def to_dict(self) -> Dict[str, object]:
        def dump(items: List[PackageChange]) -> List[Dict[str, object]]:
            out: List[Dict[str, object]] = []
            for item in sorted(items, key=lambda p: (p.name, p.source)):
                payload = {
                    "name": item.name,
                    "source": item.source or "path",
                    "change": item.change_type,
                }
                if item.old is not None:
                    payload["old"] = item.old
                if item.new is not None:
                    payload["new"] = item.new
                out.append(payload)
            return out

        return {
            "target": self.target,
            "added": dump(self.added),
            "removed": dump(self.removed),
            "updated": dump(self.updated),
        }


TARGETS = {
    "backend": "backend",
    "frontend": "frontend",
}

CHECKSUM_TARGETS = [
    Path("Cargo.toml"),
    Path("backend/Cargo.toml"),
    Path("frontend/Cargo.toml"),
    Path("shared/Cargo.toml"),
]


WORKSPACE_BLOCK_RE = re.compile(r"^\[workspace\].*?(?=^\[|\Z)", re.MULTILINE | re.DOTALL)
WORKSPACE_MEMBERS_RE = re.compile(r"^(?P<indent>\s*)members\s*=\s*\[(?P<body>.*?)\](?P<tail>[^\n]*)", re.MULTILINE | re.DOTALL)


def _format_members(indent: str, members: List[str], tail: str) -> str:
    if members:
        inner_indent = indent + "    "
        body_lines = []
        for index, member in enumerate(members):
            suffix = "," if index < len(members) - 1 else ""
            body_lines.append(f'{inner_indent}"{member}"{suffix}')
        body = "\n".join(body_lines)
        result = f"{indent}members = [\n{body}\n{indent}]"
    else:
        result = f"{indent}members = []"
    if tail:
        result += tail
    return result


def _update_workspace_members(manifest_text: str, members: List[str]) -> str:
    block_match = WORKSPACE_BLOCK_RE.search(manifest_text)
    if not block_match:
        raise RuntimeError("Cannot locate [workspace] section in root Cargo.toml")
    block = block_match.group(0)
    member_match = WORKSPACE_MEMBERS_RE.search(block)
    if not member_match:
        raise RuntimeError("Cannot locate workspace members array in root Cargo.toml")
    indent = member_match.group("indent")
    tail = member_match.group("tail")
    replacement = _format_members(indent, members, tail)
    updated_block = WORKSPACE_MEMBERS_RE.sub(replacement, block, count=1)
    return manifest_text[: block_match.start()] + updated_block + manifest_text[block_match.end():]


def write_manifest(target: str, members: Iterable[str]) -> str:
    manifest_path = ROOT / "Cargo.toml"
    manifest_text = manifest_path.read_text()
    member_list = list(members)
    if not member_list:
        member_list = []
    manifest_text = _update_workspace_members(manifest_text, member_list)
    target_dir = CHEF_DIR / target
    target_dir.mkdir(parents=True, exist_ok=True)
    (target_dir / "Cargo.toml").write_text(manifest_text)
    return manifest_text


def copy_member_manifest(tmp_root: Path, member: str) -> None:
    src_manifest = ROOT / member / "Cargo.toml"
    if not src_manifest.exists():
        raise RuntimeError(f"Missing manifest for workspace member '{member}'")
    dest_dir = tmp_root / member
    dest_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src_manifest, dest_dir / "Cargo.toml")

    src_build = ROOT / member / "build.rs"
    if src_build.exists():
        shutil.copy2(src_build, dest_dir / "build.rs")

    src_dir = dest_dir / "src"
    src_dir.mkdir(parents=True, exist_ok=True)
    manifest_data = tomllib.loads(src_manifest.read_text())

    def write_file(path: Path, content: str) -> None:
        if not path.exists():
            path.write_text(content)

    if member == "backend":
        write_file(src_dir / "main.rs", "fn main() {}\n")
        write_file(src_dir / "lib.rs", "pub fn placeholder() {}\n")
        return

    if member == "frontend":
        write_file(src_dir / "lib.rs", "pub fn placeholder() {}\n")
        write_file(src_dir / "main.rs", "fn main() {}\n")
        return

    if member == "shared":
        write_file(src_dir / "lib.rs", "pub fn placeholder() {}\n")
        return

    # Heuristic: default to library placeholder; add bin if crate declares explicit bins
    write_file(src_dir / "lib.rs", "pub fn placeholder() {}\n")
    bins = manifest_data.get("bin", [])
    if bins:
        write_file(src_dir / "main.rs", "fn main() {}\n")


def find_path_dependencies(manifest_path: Path) -> Set[str]:
    data = tomllib.loads(manifest_path.read_text())
    sections = [
        data.get("dependencies", {}),
        data.get("dev-dependencies", {}),
        data.get("build-dependencies", {}),
    ]

    targets = data.get("target", {})
    for target_table in targets.values():
        sections.append(target_table.get("dependencies", {}))
        sections.append(target_table.get("dev-dependencies", {}))
        sections.append(target_table.get("build-dependencies", {}))

    discovered: Set[str] = set()
    base_dir = manifest_path.parent

    for section in sections:
        if not isinstance(section, dict):
            continue
        for value in section.values():
            if isinstance(value, dict) and "path" in value:
                candidate = (base_dir / value["path"]).resolve()
                try:
                    rel = candidate.relative_to(ROOT.resolve())
                except ValueError:
                    continue
                discovered.add(rel.as_posix())
    return discovered


def resolve_members(initial: str) -> List[str]:
    root_dir = ROOT.resolve()
    stack = [initial]
    visited: Set[str] = set()
    ordered: List[str] = []

    while stack:
        member = stack.pop()
        if member in visited:
            continue
        visited.add(member)
        ordered.append(member)
        manifest = root_dir / member / "Cargo.toml"
        if not manifest.exists():
            raise RuntimeError(f"Missing manifest at {manifest}")
        deps = sorted(find_path_dependencies(manifest), reverse=True)
        for dep in deps:
            stack.append(dep)
    return ordered


def generate_lockfile(target: str, manifest_text: str, members: Iterable[str]) -> Tuple[Path, Path]:
    target_dir = CHEF_DIR / target
    lock_path = target_dir / "Cargo.lock"
    with tempfile.TemporaryDirectory(prefix=f"cargo-chef-{target}-") as tmp_dir:
        tmp_root = Path(tmp_dir)
        (tmp_root / "Cargo.toml").write_text(manifest_text)
        for member in members:
            copy_member_manifest(tmp_root, member)
        result = subprocess.run(
            ["cargo", "generate-lockfile", "--manifest-path", str(tmp_root / "Cargo.toml")],
            check=False,
            cwd=tmp_root,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            sys.stderr.write(result.stdout)
            sys.stderr.write(result.stderr)
            raise RuntimeError(f"cargo generate-lockfile failed for {target}")
        shutil.copy2(tmp_root / "Cargo.lock", lock_path)
    return target_dir / "Cargo.toml", lock_path


def parse_packages(lock_path: Path) -> Dict[Tuple[str, str], str]:
    packages: Dict[Tuple[str, str], str] = {}
    if not lock_path.exists():
        return packages
    current: Dict[str, str] | None = None
    for raw_line in lock_path.read_text().splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if line == "[[package]]":
            if current and "name" in current and "version" in current:
                key = (current["name"], current.get("source", ""))
                packages[key] = current["version"]
            current = {}
            continue
        if line.startswith("[[") and line != "[[package]]":
            if current and "name" in current and "version" in current:
                key = (current["name"], current.get("source", ""))
                packages[key] = current["version"]
            current = None
            continue
        if line.startswith("[") and not line.startswith("[["):
            if current and "name" in current and "version" in current:
                key = (current["name"], current.get("source", ""))
                packages[key] = current["version"]
            current = None
            continue
        if current is None:
            continue
        if "=" not in line:
            continue
        key, value = (part.strip() for part in line.split("=", 1))
        if value.startswith("\"") and value.endswith("\""):
            value = value[1:-1]
        current[key] = value
    if current and "name" in current and "version" in current:
        key = (current["name"], current.get("source", ""))
        packages[key] = current["version"]
    return packages


def diff_packages_from_maps(target: str, old_packages: Dict[Tuple[str, str], str], new_packages: Dict[Tuple[str, str], str]) -> TargetSummary:
    added: List[PackageChange] = []
    removed: List[PackageChange] = []
    updated: List[PackageChange] = []

    for key, new_version in new_packages.items():
        old_version = old_packages.get(key)
        name, source = key
        if old_version is None:
            added.append(PackageChange(name, source, "added", new=new_version))
        elif new_version != old_version:
            updated.append(PackageChange(name, source, "updated", old=old_version, new=new_version))

    for key, old_version in old_packages.items():
        if key not in new_packages:
            name, source = key
            removed.append(PackageChange(name, source, "removed", old=old_version))

    return TargetSummary(target=target, added=added, removed=removed, updated=updated)


def ensure_cargo_available() -> None:
    if shutil.which("cargo") is None:
        raise RuntimeError("cargo binary not found in PATH")


def compute_sha256(path: Path) -> str:
    if not path.exists():
        raise RuntimeError(f"Cannot compute checksum; missing file: {path}")
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def write_checksums() -> Dict[str, str]:
    data: Dict[str, str] = {}
    for rel in CHECKSUM_TARGETS:
        abs_path = ROOT / rel
        data[str(rel)] = compute_sha256(abs_path)
    CHECKSUM_PATH.parent.mkdir(parents=True, exist_ok=True)
    CHECKSUM_PATH.write_text(json.dumps(data, indent=2, sort_keys=True))
    return data


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--summary-path",
        type=Path,
        default=Path("cargo-chef-summary.json"),
        help="Where to write JSON summary of dependency changes.",
    )
    args = parser.parse_args()

    ensure_cargo_available()

    summaries: List[TargetSummary] = []
    for target, root_member in TARGETS.items():
        members = resolve_members(root_member)
        manifest_text = write_manifest(target, members)
        target_dir = CHEF_DIR / target
        old_snapshot = parse_packages(target_dir / "Cargo.lock")
        _, new_lock = generate_lockfile(target, manifest_text, members)
        summary = diff_packages_from_maps(target, old_snapshot, parse_packages(new_lock))
        summaries.append(summary)

    args.summary_path.write_text(
        json.dumps([summary.to_dict() for summary in summaries], indent=2, sort_keys=True)
    )
    write_checksums()
    return 0


if __name__ == "__main__":
    sys.exit(main())
