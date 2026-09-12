#!/usr/bin/env python3
"""Package the checked Apple Silicon binary as a drag-and-drop macOS disk image."""

import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile


def package(root):
    binary = root / "target/aarch64-apple-darwin/release/voice-journal"
    output = root / "voice-journal-aarch64-apple-darwin.dmg"
    if not binary.is_file():
        raise FileNotFoundError("Build the aarch64-apple-darwin release binary first")
    if output.exists():
        raise FileExistsError(f"Refusing to overwrite {output}")
    subprocess.run(["lipo", "-verify_arch", "arm64", str(binary)], check=True)
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version=1", "--offline", "--locked"],
        cwd=root, text=True,
    ))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "voice-journal")

    with tempfile.TemporaryDirectory(prefix="voice-journal-dmg-") as temporary:
        staging = Path(temporary) / "volume"
        app = staging / "Voice Journal.app"
        contents = app / "Contents"
        executable_dir = contents / "MacOS"
        resources = contents / "Resources"
        executable_dir.mkdir(parents=True)
        resources.mkdir()
        executable = executable_dir / "voice-journal"
        shutil.copy2(binary, executable)
        executable.chmod(0o755)
        shutil.copy2(root / "scripts/install-launchd.sh", resources)
        shutil.copy2(root / "docs/INSTALL.txt", resources)
        shutil.copy2(root / "LICENSE", resources)
        shutil.copy2(root / "docs/INSTALL.txt", staging)
        (staging / "Applications").symlink_to("/Applications", target_is_directory=True)
        with (contents / "Info.plist").open("wb") as plist:
            plistlib.dump({
                "CFBundleDevelopmentRegion": "en",
                "CFBundleDisplayName": "Voice Journal",
                "CFBundleExecutable": "voice-journal",
                "CFBundleIdentifier": "com.johnson.voice-journal",
                "CFBundleInfoDictionaryVersion": "6.0",
                "CFBundleName": "Voice Journal",
                "CFBundlePackageType": "APPL",
                "CFBundleShortVersionString": version,
                "CFBundleVersion": version,
                "LSMinimumSystemVersion": "14.0",
                "LSUIElement": True,
                "NSDocumentsFolderUsageDescription": "Voice Journal saves your transcripts in your Documents folder.",
            }, plist)
        subprocess.run(["plutil", "-lint", str(contents / "Info.plist")], check=True)
        subprocess.run(["codesign", "--force", "--sign", "-", str(app)], check=True)
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
        subprocess.run([
            "hdiutil", "create", "-volname", "Voice Journal", "-srcfolder", str(staging),
            "-fs", "HFS+", "-format", "UDZO", str(output),
        ], check=True)
        subprocess.run(["hdiutil", "verify", str(output)], check=True)
    return output


if __name__ == "__main__":
    print(package(Path(__file__).resolve().parents[1]))
