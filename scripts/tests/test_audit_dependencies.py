"""Audit warnings without advisory metadata must still block Windows. Author: jaswtao · Date: 2026-10-24."""
import contextlib
import io
from pathlib import Path
import runpy
import unittest

review = runpy.run_path(str(Path(__file__).resolve().parents[1] / "audit-dependencies.py"))["review_report"]


class AuditReportTests(unittest.TestCase):
    def test_yanked_package_without_advisory_blocks_only_when_reachable(self):
        report = {"vulnerabilities": {"list": []}, "warnings": {"yanked": [
            {"kind": "yanked", "package": {"name": "clipboard-win", "version": "5.4.1"}, "advisory": None}
        ]}}
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(review(report, {"clipboard-win"}), ["yanked:clipboard-win@5.4.1"])
            self.assertEqual(review(report, set()), [])

    def test_vulnerabilities_always_block_and_windows_warnings_block(self):
        finding = {"kind": "unsound", "package": {"name": "tokio", "version": "1.53.2"}, "advisory": {"id": "example"}}
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(review({"vulnerabilities": {"list": [finding]}, "warnings": {}}, set()), ["example"])
            self.assertEqual(review({"vulnerabilities": {"list": []}, "warnings": {"unsound": [finding]}}, {"tokio"}), ["example"])


if __name__ == "__main__":
    unittest.main()
