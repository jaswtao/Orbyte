"""Exercise packaging guards with real Git and stubbed builds. Author: jaswtao · Date: 2026-10-20."""
import hashlib
import os
import shutil
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
INSTALLER = "Orbyte_0.1.0_x64-setup.exe"


class PrepareReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="orbyte-release-test-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.repo = self.directory / "source"
        (self.repo / "scripts").mkdir(parents=True)
        (self.repo / "apps/desktop/src-tauri").mkdir(parents=True)
        shutil.copy2(ROOT / "scripts/prepare-release.ps1", self.repo / "scripts")
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_LICENSES.json"):
            (self.repo / name).write_text("fixture\n", encoding="utf-8")
        (self.repo / ".gitignore").write_text("/release/\n/target/\n/custom-target/\n", encoding="utf-8")
        self.git("init", "--quiet")
        self.git("config", "core.autocrlf", "false")
        self.git("config", "user.name", "jaswtao")
        self.git("config", "user.email", "8916109+jaswtao@users.noreply.github.com")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "Packaging fixture")
        self.runner = self.directory / "run.ps1"
        self.runner.write_text(r"""
$ErrorActionPreference = 'Stop'
function global:python {
    $global:LASTEXITCODE = 0
    if ($args -contains '--mpl-sources') {
        Set-Content -LiteralPath $args[($args.IndexOf('--mpl-sources') + 1)] -Value 'fixture MPL sources'
    }
}
function global:cargo {
    $global:LASTEXITCODE = 0
    $target = if ($env:CARGO_TARGET_DIR) {
        [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR, (Get-Location).Path)
    } else { $env:ORBYTE_TEST_TARGET }
    @{ target_directory = $target } | ConvertTo-Json -Compress
}
function global:pnpm {
    $global:LASTEXITCODE = 0
    if ($env:ORBYTE_TEST_SKIP_BUILD -eq '1') { return }
    # Tauri invokes Cargo from its Rust project directory.
    Push-Location (Join-Path $env:ORBYTE_TEST_REPO 'apps/desktop/src-tauri')
    try {
        $target = if ($env:CARGO_TARGET_DIR) {
            [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR, (Get-Location).Path)
        } else { $env:ORBYTE_TEST_TARGET }
        $destination = Join-Path $target 'release/bundle/nsis'
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        Set-Content -LiteralPath (Join-Path $destination 'Orbyte_0.1.0_x64-setup.exe') -Value 'new installer'
    } finally { Pop-Location }
}
$originalTarget = $env:CARGO_TARGET_DIR
try {
    & (Join-Path $env:ORBYTE_TEST_REPO 'scripts/prepare-release.ps1') -Tag v0.1.0
} finally {
    if ($env:CARGO_TARGET_DIR -cne $originalTarget) { throw 'Cargo target environment was not restored.' }
}
""", encoding="utf-8")

    def git(self, *arguments):
        return subprocess.run(["git", *arguments], cwd=self.repo, check=True,
                              capture_output=True, text=True).stdout.strip()

    def package(self, target="target", skip_build=False, relative_target=False):
        environment = dict(os.environ, ORBYTE_TEST_REPO=str(self.repo),
                           ORBYTE_TEST_TARGET=str(self.repo / target),
                           ORBYTE_TEST_SKIP_BUILD="1" if skip_build else "0")
        environment.pop("CARGO_TARGET_DIR", None)
        if target != "target" or relative_target:
            environment["CARGO_TARGET_DIR"] = target if relative_target else str(self.repo / target)
        return subprocess.run(["pwsh", "-NoProfile", "-File", str(self.runner)],
                              cwd=self.repo, env=environment, capture_output=True,
                              text=True, encoding="utf-8", errors="replace")

    def test_missing_tag_is_rejected_before_creating_release(self):
        result = self.package()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must exist before packaging", result.stderr)
        self.assertFalse((self.repo / "release").exists())

    def test_tag_on_another_commit_is_rejected(self):
        self.git("tag", "v0.1.0")
        self.git("commit", "--quiet", "--allow-empty", "-m", "Another commit")
        result = self.package()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must point to HEAD", result.stderr)

    def test_dirty_source_is_rejected(self):
        self.git("tag", "v0.1.0")
        (self.repo / "LICENSE").write_text("uncommitted", encoding="utf-8")
        result = self.package()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Commit the reviewed source", result.stderr)

    def test_annotated_tag_packages_matching_source_and_checksums(self):
        self.git("tag", "-a", "v0.1.0", "-m", "Preview")
        result = self.package()
        self.assertEqual(result.returncode, 0, result.stderr)
        output = self.repo / "release/v0.1.0"
        self.assertEqual((output / INSTALLER).read_text().strip(), "new installer")
        with zipfile.ZipFile(output / "Orbyte-v0.1.0-source.zip") as archive:
            committed = subprocess.check_output(["git", "show", "HEAD:LICENSE"], cwd=self.repo)
            self.assertEqual(archive.read("Orbyte-0.1.0/LICENSE"), committed)
        for line in (output / "SHA256SUMS.txt").read_text(encoding="utf-8-sig").splitlines():
            digest, name = line.split("  ", 1)
            self.assertEqual(digest, hashlib.sha256((output / name).read_bytes()).hexdigest())

    def test_custom_target_ignores_old_default_installer(self):
        self.git("tag", "v0.1.0")
        old = self.repo / "target/release/bundle/nsis" / INSTALLER
        old.parent.mkdir(parents=True)
        old.write_text("old default installer", encoding="utf-8")
        result = self.package(target="custom-target")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.repo / "release/v0.1.0" / INSTALLER).read_text().strip(), "new installer")
        self.assertEqual(old.read_text(), "old default installer")

    def test_successful_command_without_new_installer_is_rejected(self):
        self.git("tag", "v0.1.0")
        old = self.repo / "custom-target/release/bundle/nsis" / INSTALLER
        old.parent.mkdir(parents=True)
        old.write_text("stale installer", encoding="utf-8")
        result = self.package(target="custom-target", skip_build=True, relative_target=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Expected installer was not produced", result.stderr)
        self.assertFalse((self.repo / "release/v0.1.0" / INSTALLER).exists())

    def test_relative_target_is_shared_with_tauri_build(self):
        self.git("tag", "v0.1.0")
        old = self.repo / "target/post-mvp-check/release/bundle/nsis" / INSTALLER
        old.parent.mkdir(parents=True)
        old.write_text("stale installer", encoding="utf-8")
        result = self.package(target="target/post-mvp-check", relative_target=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(old.read_text().strip(), "new installer")
        self.assertEqual((self.repo / "release/v0.1.0" / INSTALLER).read_text().strip(), "new installer")
        self.assertFalse((self.repo / "apps/desktop/src-tauri/target").exists())


if __name__ == "__main__":
    unittest.main()
