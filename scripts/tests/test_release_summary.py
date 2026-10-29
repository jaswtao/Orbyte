"""Validate bundled release notes before packaging. Author: jaswtao · Date: 2026-10-29."""
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("check_release", ROOT / "scripts/check-release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseSummaryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.path = self.root / "doc/releases/v0.3.0.json"
        self.path.parent.mkdir(parents=True)
        self.summary = json.loads((ROOT / "doc/releases/v0.3.0.json").read_text(encoding="utf-8"))
        patcher = patch.object(release, "ROOT", self.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def validate(self):
        self.path.write_text(json.dumps(self.summary), encoding="utf-8")
        release.check_release_summary("0.3.0")

    def test_matching_bilingual_summary(self):
        self.validate()

    def test_missing_summary(self):
        with self.assertRaisesRegex(ValueError, "Missing bundled release summary"):
            release.check_release_summary("0.3.0")

    def test_version_mismatch(self):
        self.summary["version"] = "0.2.0"
        with self.assertRaisesRegex(ValueError, "match the application version"):
            self.validate()

    def test_missing_translation(self):
        for locale in ("en", "zh"):
            with self.subTest(locale=locale):
                notes = self.summary.pop(locale)
                with self.assertRaisesRegex(ValueError, f"requires {locale} notes"):
                    self.validate()
                self.summary[locale] = notes

    def test_invalid_content(self):
        for content in (None, "text", [""], ["  "], [1]):
            with self.subTest(content=content):
                self.summary["zh"]["features"] = content
                with self.assertRaisesRegex(ValueError, "non-empty strings"):
                    self.validate()

    def test_empty_language(self):
        self.summary["en"] = {"features": [], "fixes": []}
        with self.assertRaisesRegex(ValueError, "requires content for en"):
            self.validate()

    def test_fix_only_release(self):
        self.summary["en"]["features"] = []
        self.validate()

    def test_invalid_date(self):
        for value in ("", "2026-02-30", "20261029"):
            with self.subTest(value=value):
                self.summary["date"] = value
                with self.assertRaises(ValueError):
                    self.validate()
