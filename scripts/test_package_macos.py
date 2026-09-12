"""Check bundle contents and failure handling without requiring macOS tools."""

import importlib.util
import json
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("package_macos", Path(__file__).with_name("package-macos.py"))
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in ["target/aarch64-apple-darwin/release/voice-journal",
                     "scripts/install-launchd.sh", "docs/INSTALL.txt", "LICENSE"]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name)
        self.output = self.root / "voice-journal-aarch64-apple-darwin.dmg"
        self.calls = []
        self.metadata = patch.object(packaging.subprocess, "check_output", return_value=json.dumps({
            "packages": [{"name": "voice-journal", "version": "0.1.0"}]
        }))
        self.metadata.start()
        self.addCleanup(self.metadata.stop)

    def tool(self, args, **kwargs):
        self.calls.append(args)
        self.assertTrue(kwargs["check"])
        if args[0] == "lipo":
            self.assertTrue(Path(args[1]).is_file())
            self.assertEqual(args[2:], ["-verify_arch", "arm64"])
        if args[:2] == ["hdiutil", "create"]:
            staging = Path(args[args.index("-srcfolder") + 1])
            app = staging / "Voice Journal.app/Contents"
            info = plistlib.loads((app / "Info.plist").read_bytes())
            self.assertEqual(info["CFBundleExecutable"], "voice-journal")
            self.assertEqual(info["CFBundleIdentifier"], "com.johnson.voice-journal")
            self.assertEqual(info["CFBundleShortVersionString"], "0.1.0")
            self.assertEqual(info["LSMinimumSystemVersion"], "14.0")
            self.assertIs(info["LSUIElement"], True)
            binary = app / "MacOS/voice-journal"
            self.assertEqual(binary.read_bytes(), (self.root / "target/aarch64-apple-darwin/release/voice-journal").read_bytes())
            self.assertEqual(binary.stat().st_mode & 0o777, 0o755)
            self.assertEqual((staging / "Applications").readlink(), Path("/Applications"))
            self.assertEqual((staging / "INSTALL.txt").read_bytes(), (app / "Resources/INSTALL.txt").read_bytes())
            self.assertTrue((app / "Resources/install-launchd.sh").is_file())
            self.assertTrue((app / "Resources/LICENSE").is_file())
            self.staging = staging
            Path(args[-1]).write_bytes(b"mock disk image")

    def test_bundle_and_verified_image(self):
        with patch.object(packaging.subprocess, "run", side_effect=self.tool):
            self.assertEqual(packaging.package(self.root), self.output)
        self.assertEqual(self.calls[-1], ["hdiutil", "verify", str(self.output)])
        self.assertFalse(self.staging.exists(), "temporary staging should be cleaned")
        self.assertTrue(self.output.is_file())

    def test_missing_binary_stops_before_tools(self):
        (self.root / "target/aarch64-apple-darwin/release/voice-journal").unlink()
        with patch.object(packaging.subprocess, "run") as run:
            with self.assertRaises(FileNotFoundError):
                packaging.package(self.root)
            run.assert_not_called()

    def test_existing_download_is_preserved(self):
        self.output.write_bytes(b"existing download")
        with patch.object(packaging.subprocess, "run") as run:
            with self.assertRaises(FileExistsError):
                packaging.package(self.root)
            run.assert_not_called()
        self.assertEqual(self.output.read_bytes(), b"existing download")

    def test_wrong_architecture_stops_packaging(self):
        with patch.object(packaging.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "lipo")) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                packaging.package(self.root)
            self.assertEqual(run.call_count, 1)
        self.assertFalse(self.output.exists())

    def test_signing_and_image_failures_propagate(self):
        for failed in [["codesign", "--force"], ["codesign", "--verify"], ["hdiutil", "create"], ["hdiutil", "verify"]]:
            with self.subTest(failed=failed):
                self.calls = []
                if self.output.exists():
                    self.output.unlink()

                def fail(args, **kwargs):
                    if args[:2] == failed:
                        raise subprocess.CalledProcessError(1, args)
                    self.tool(args, **kwargs)

                with patch.object(packaging.subprocess, "run", side_effect=fail):
                    with self.assertRaises(subprocess.CalledProcessError):
                        packaging.package(self.root)
                if failed != ["hdiutil", "verify"]:
                    self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
