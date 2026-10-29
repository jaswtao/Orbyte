"""Validate release metadata before building. Author: jaswtao · Date: 2026-10-20."""
import argparse
import json
import re
import tomllib
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def check_release_summary(version):
    path = ROOT / "doc" / "releases" / f"v{version}.json"
    if not path.is_file():
        raise ValueError(f"Missing bundled release summary: {path.name}")
    summary = json.loads(path.read_text(encoding="utf-8"))
    if summary.get("version") != version:
        raise ValueError("Release summary must match the application version.")
    released = summary.get("date", "")
    if not isinstance(released, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", released):
        raise ValueError("Release summary date must use YYYY-MM-DD.")
    date.fromisoformat(released)
    for locale in ("en", "zh"):
        notes = summary.get(locale)
        if not isinstance(notes, dict):
            raise ValueError(f"Release summary requires {locale} notes.")
        for section in ("features", "fixes"):
            items = notes.get(section)
            if not isinstance(items, list) or any(not isinstance(item, str) or not item.strip() for item in items):
                raise ValueError(f"Release summary {locale}.{section} must contain non-empty strings.")
        if not notes["features"] and not notes["fixes"]:
            raise ValueError(f"Release summary requires content for {locale}.")


def check(tag=None):
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    root = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
    frontend = json.loads((ROOT / "apps/desktop/package.json").read_text(encoding="utf-8"))
    tauri = json.loads((ROOT / "apps/desktop/src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    version = workspace["workspace"]["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Expected a stable x.y.z package version; use GitHub prerelease status for preview builds.")
    if root["version"] != version or frontend["version"] != version or tauri["version"] != version:
        raise ValueError("Root, frontend, Rust and Tauri versions must agree.")
    if tag and tag != f"v{version}":
        raise ValueError(f"Tag must be v{version}, received {tag}.")
    check_release_summary(version)
    license_text = (ROOT / "LICENSE").read_text(encoding="utf-8")
    if "Copyright (c) 2026 jaswtao" not in license_text:
        raise ValueError("Unexpected copyright holder.")
    for package in [root, frontend, workspace["workspace"]["package"]]:
        if package["license"] != "MIT":
            raise ValueError("First-party package license must be MIT.")
    for manifest in [*ROOT.glob("crates/*/Cargo.toml"), ROOT / "apps/desktop/src-tauri/Cargo.toml"]:
        package = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]
        if package.get("license") != {"workspace": True} or package.get("repository") != {"workspace": True}:
            raise ValueError(f"Missing inherited license/repository: {manifest.relative_to(ROOT)}")
    if tauri["bundle"]["license"] != "MIT":
        raise ValueError("Bundle license must be MIT.")
    print(f"Release metadata OK: {version}")
    return version


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag")
    args = parser.parse_args()
    check(args.tag)
