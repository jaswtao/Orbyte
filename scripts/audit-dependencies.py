"""Reject vulnerabilities and warnings affecting Windows. Author: jaswtao · Date: 2026-10-20."""
import json
import runpy
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
collect = runpy.run_path(str(ROOT / "scripts/generate-licenses.py"))["collect"]
records = collect()
active = {r["name"] for r in records if r["ecosystem"] == "cargo"}
# This exemption is valid only while SQLx MySQL/RSA stays outside the Windows build.
if "rsa" in active:
    sys.exit("RSA is now reachable: remove RUSTSEC-2023-0071 exemption and reassess.")
result = subprocess.run(["cargo", "audit", "--json"], cwd=ROOT, stdout=subprocess.PIPE,
                        encoding="utf-8", check=False)
if result.returncode not in (0, 1):
    sys.exit("cargo-audit did not complete.")
try:
    report = json.loads(result.stdout)
except json.JSONDecodeError:
    sys.exit("cargo-audit did not return an audit report.")
if not {"vulnerabilities", "warnings"}.issubset(report):
    sys.exit("Incomplete cargo-audit report.")
failures = []
for warning in [*report["vulnerabilities"].get("list", []),
                *(w for group in report["warnings"].values() for w in group)]:
    name = warning["package"]["name"]
    advisory = warning["advisory"]["id"]
    affected = name in active
    print(f"{advisory}: {name} ({'Windows dependency: BLOCKED' if affected else 'not in Windows build'})")
    if affected or warning in report["vulnerabilities"].get("list", []):
        failures.append(advisory)
if failures:
    sys.exit("Review required: " + ", ".join(failures))
if result.returncode:
    sys.exit("cargo-audit failed; do not publish.")
print("Rust audit passed; RSA is absent from the Windows dependency graph.")
