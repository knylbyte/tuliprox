#!/usr/bin/env python3
"""Generate cargo-chef manifests and lockfiles for backend/frontend workspaces."""
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Tuple

ROOT = Path(__file__).resolve().parent.parent
CHEF_DIR = ROOT / "docker" / "build-tools" / "cargo-chef"
MEMBERS_TEMPLATE = 'members = ["backend", "frontend", "shared"]'


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
    "backend": ["backend", "shared"],
    "frontend": ["frontend", "shared"],
}


def write_manifest(target: str, members: List[str]) -> str:
    manifest_text = (ROOT / "Cargo.toml").read_text()
    members_list = ', '.join(f'"{m}"' for m in members)
    replacement = f"members = [{members_list}]"
    if MEMBERS_TEMPLATE not in manifest_text:
        raise RuntimeError(f"Cannot find workspace members template in root Cargo.toml for {target}")
    manifest_text = manifest_text.replace(MEMBERS_TEMPLATE, replacement, 1)
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
    if member == "backend":
        (src_dir / "main.rs").write_text("fn main() {}\n")
    if member == "frontend":
        # Provide both lib.rs and main.rs so cargo can satisfy either target type
        (src_dir / "lib.rs").write_text("pub fn placeholder() {}\n")
        (src_dir / "main.rs").write_text("fn main() {}\n")
    if member == "shared":
        (src_dir / "lib.rs").write_text("pub fn placeholder() {}\n")


def generate_lockfile(target: str, manifest_text: str, members: List[str]) -> Tuple[Path, Path]:
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
    for target, members in TARGETS.items():
        manifest_text = write_manifest(target, members)
        target_dir = CHEF_DIR / target
        old_snapshot = parse_packages(target_dir / "Cargo.lock")
        _, new_lock = generate_lockfile(target, manifest_text, members)
        summary = diff_packages_from_maps(target, old_snapshot, parse_packages(new_lock))
        summaries.append(summary)

    args.summary_path.write_text(
        json.dumps([summary.to_dict() for summary in summaries], indent=2, sort_keys=True)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
