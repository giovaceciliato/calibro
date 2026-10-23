"""Regression coverage for GitHub release tags and download completeness."""

import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("prepare_release", ROOT / "packaging/prepare-release.py")
RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE)


class ReleaseDownloadsTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.assets = Path(self.tmp.name)
        self.names = [
            "calibro-windows-x86_64.exe", "calibro-windows-x86_64.zip",
            "calibro-0.1.0-1.x86_64.rpm", "calibro-0.1.0-linux-x86_64.tar.gz",
            "calibro-macos-arm64.tar.gz", "calibro-linux-x86_64.tar.gz",
        ]
        for name in self.names:
            (self.assets / name).write_bytes(f"fixture: {name}".encode())

    def prepare(self, tag):
        return RELEASE.prepare(tag, "giovaceciliato/calibro", self.assets, ROOT / "RELEASE_NOTES.md")

    def test_release_tags_with_and_without_version_prefix(self):
        # 0.1.1 reproduces the published release that failed in GitHub Actions.
        for tag in ["0.1.1", "v0.1.1", "beta", "preview/0.1.1"]:
            with self.subTest(tag=tag):
                body = self.prepare(tag)
                for name in self.names:
                    self.assertIn(f"/releases/download/{quote(tag, safe='')}/{name}", body)
                self.assertIn("## Launching the app", body)
                self.assertIn("## Known limitations", body)

    def test_empty_tag_is_rejected(self):
        for tag in ["", "   "]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                self.prepare(tag)

    def test_checksums_cover_every_desktop_download(self):
        self.prepare("0.1.1")
        lines = (self.assets / "SHA256SUMS.txt").read_text().splitlines()
        self.assertEqual(len(lines), len(self.names))
        for line in lines:
            digest, name = line.split("  ")
            self.assertEqual(digest, hashlib.sha256((self.assets / name).read_bytes()).hexdigest())

    def test_missing_empty_and_ambiguous_assets_are_rejected(self):
        exe = self.assets / self.names[0]
        exe.unlink()
        with self.assertRaises(ValueError):
            self.prepare("0.1.1")
        exe.touch()
        with self.assertRaises(ValueError):
            self.prepare("0.1.1")
        exe.write_bytes(b"fixture")
        (self.assets / "calibro-0.2.0-1.x86_64.rpm").write_bytes(b"duplicate")
        with self.assertRaises(ValueError):
            self.prepare("0.1.1")


if __name__ == "__main__":
    unittest.main()
