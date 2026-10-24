"""Prepare an isolated original watcher plus rebuild counters. Author: jaswtao · Date: 2026-10-24."""
from pathlib import Path
import re
import shutil
import subprocess
import sys
import uuid

root = Path(__file__).resolve().parents[1]
reference = sys.argv[1]
# Resolve before copying; the resolved hash cannot be interpreted as a git option.
commit = subprocess.check_output(["git", "rev-parse", "--verify", "--end-of-options", reference + "^{commit}"], cwd=root, text=True).strip()
destination = root / ".tmp" / ("index-baseline-" + uuid.uuid4().hex)
destination.mkdir(parents=True)
for crate in ("orbyte-core", "orbyte-files"):
    shutil.copytree(root / "crates" / crate, destination / "crates" / crate)
manifest = (root / "Cargo.toml").read_text(encoding="utf-8")
manifest = re.sub(r"members = \[[\s\S]*?\]", 'members = ["crates/orbyte-core", "crates/orbyte-files"]', manifest, count=1)
(destination / "Cargo.toml").write_text(manifest, encoding="utf-8")
shutil.copyfile(root / "Cargo.lock", destination / "Cargo.lock")
for filename in ("index.rs", "watcher.rs"):
    content = subprocess.check_output(["git", "show", f"{commit}:crates/orbyte-files/src/{filename}"], cwd=root).decode("utf-8")
    if filename == "watcher.rs":
        content = content.replace("AtomicBool, Ordering", "AtomicBool, AtomicU64, Ordering")
        content = content.replace("    index: Mutex<Option<FileIndex>>,", "    index: Mutex<Option<FileIndex>>,\n    rebuilds: AtomicU64,")
        content = content.replace("    pub fn search(&self, query:", "    pub fn rebuild_count(&self) -> u64 { self.rebuilds.load(Ordering::Relaxed) }\n    pub fn search(&self, query:")
        content = content.replace("Ok(next) => *index = Some(next),", "Ok(next) => { *index = Some(next); self.rebuilds.fetch_add(1, Ordering::Relaxed); },")
        content = content.replace("            index: Mutex::new(None),", "            index: Mutex::new(None),\n            rebuilds: AtomicU64::new(0),")
    (destination / "crates/orbyte-files/src" / filename).write_text(content, encoding="utf-8")
print(destination / "Cargo.toml")
