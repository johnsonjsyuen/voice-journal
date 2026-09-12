# Commit releases (Implementation)

## Contract

Users download an Apple Silicon macOS DMG or the existing archive from GitHub Releases.
Each successful `main` push build publishes one release tagged `main-<full SHA>`
at that exact commit, after lint, Linux tests, and macOS checks pass. Pull requests
never publish. This applies to the tip commit of each push; push commits individually
to obtain a build and release for each commit. Historical commits are not backfilled.

Keep packaging in the [CI workflow](../.github/workflows/ci.yml); the release job
downloads its artifact without rebuilding. Use the explicit Apple Silicon target.
Use job-scoped `contents: write` and the built-in token. Serialize only runs for
the same SHA, without canceling them. Published releases are retained on reruns;
unfinished drafts have their asset replaced before publication. GitHub CLI creates
the initial draft, uploads the archive, and publishes it. No version bumps, signing,
notarization, other platforms, or external credentials are introduced. The app
bundle uses ad-hoc signing for local code integrity, without a Developer ID.

## DMG packaging

After the existing macOS build, `python3 scripts/package-macos.py` packages
`target/aarch64-apple-darwin/release/voice-journal` in `Voice Journal.app`.
Read the version from offline locked Cargo metadata; include the executable,
bundle metadata (identifier `com.johnson.voice-journal`, agent/menu-bar behavior,
minimum macOS 14.0), installation guide, license, and existing launchd helper.
Validate the plist and ARM architecture (`lipo <binary> -verify_arch arm64`;
the input must precede the variable-length architecture list), ad-hoc sign and verify the app, then
create and verify a compressed read-only HFS+ DMG with `hdiutil`.
Use an isolated temporary staging directory and refuse to overwrite output.

The volume contains the app, an `/Applications` symlink, and `INSTALL.txt` from
[the installation guide](INSTALL.txt). The output is
`voice-journal-aarch64-apple-darwin.dmg`. CI mounts it read-only, verifies contents,
checks the bundled executable with `--check`, and always detaches the volume.
Upload both the archive and DMG in the existing Actions artifact and require both
before creating or resuming a release. Published releases remain untouched.

The README points to the DMG and guide; retain archive instructions as an
alternative. Installation is drag-and-drop, with no privileged installer. Changes
to recording behavior, custom artwork, and automatic updates are out of scope.

## Anti-patterns

| Avoid | Required behavior |
|---|---|
| Rolling tags | Full commit SHA in each tag |
| Rebuilding in publication | Download the checked build artifact |
| Publishing from PRs | Require push to main |
| Canceling older commit builds | Concurrency scoped to commit |
| Overwriting published downloads on rerun | Reuse published release |
| Granting every job write access | Grant only the release job write access |

## Test case specifications

Static checks cover the main-only condition, three prerequisite jobs, write
permission scope, artifact name matching, explicit target, and SHA concurrency.
Exercise the release shell with a fake GitHub CLI for these integration cases:
absent release creates with exact SHA and archive; published release is skipped;
draft release uploads and publishes; create/upload failures fail the job.
Validate workflow syntax with actionlint when available and shell syntax with Bash.
Packaging checks exercise bundle metadata/resources, missing binary, wrong
architecture, refusal to overwrite, and signing/image failures with mocked macOS
tools on Linux. The macOS CI job verifies and mounts the actual generated DMG.
Hosted validation requires merging to main and checking the resulting release,
tag target and downloadable archive. macOS runtime behavior remains covered by
the existing macOS CI job.

## Error handling

| Failure | Behavior and recovery |
|---|---|
| Lint/test/build fails | No release; fix and push or rerun the failed build |
| Artifact missing | Download/publish fails; rerun build |
| GitHub lookup fails | Creation also fails for auth/network errors or an existing tag; inspect job logs and rerun |
| Upload/publication fails | Job fails; rerun resumes draft |
| Published release already exists | Successful no-op; preserve assets |
| Packaging input or macOS tool fails | Fail packaging and withhold release; temporary staging is cleaned |
| macOS blocks first open | Guide links Apple's per-app Open Anyway process for trusted downloads |

## References

- [Downloads and installation](../README.md#download)
- [GitHub CLI release creation](https://cli.github.com/manual/gh_release_create)
- [GitHub CLI draft editing](https://cli.github.com/manual/gh_release_edit)

Clarity review: all 13 checks pass; understandability 9/10. The remaining external
verification is the hosted build and release after integration.
