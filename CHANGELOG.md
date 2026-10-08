# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

First public version (0.1.0), not yet released.

The final GitHub identity is `CraftHubLauncher/CraftHub`; the application migrates the provisional
per-user data folder on first start without moving installed Craft applications.

### Added

- Catalog of the 12 Craft apps plus ArtCraft (listed separately, not installable).
- Release discovery from the GitHub REST API with ETag caching, stable/beta channels, semver
  ordering, rate-limit handling and offline use of cached data.
- Transactional install and update of audited Windows x64 portable releases:
  - mandatory SHA-256 check against GitHub metadata, cross-checked with `SHA256SUMS.txt`;
  - hardened ZIP extraction and x64 executable verification;
  - staged activation, with one previous version kept for rollback;
  - crash recovery.
- Open, update, update all, roll back, install a specific version, and uninstall with
  manifest-based deletion that keeps user data.
- Per-app cross-process locks, so the app and the command-line tool never modify the same app at once.
- System tray. Closing the window never interrupts an install, and quitting asks before
  cancelling running work.
- Native Windows update notifications (once per version, click opens the app in CraftHub).
- Update modes Manual, Notify and Automatic (installs only for apps that are closed).
- Choice of install folder for new installs, with safety validation. Existing installs stay in place.
- Self-update integration (Tauri updater with signature verification). Active only in builds made
  with an update-signing key.
- Retry for failed installs and updates, an offline banner, and keyboard focus handling in dialogs.
- Official app icons where upstream licenses them (see ASSETS.md).
- `crafthub-cli` for scripting and smoke tests.
- Release build script that keeps local paths out of binaries at build time and verifies the result.

### Security

- Self-update stays disabled unless the bundle identifier is final, a minisign public key is embedded
  and the update endpoint belongs to the same GitHub owner (enforced in code and in the release workflow).
- `scripts/privacy-scan.ps1` (self-test, repository and release modes) runs in CI and the release
  workflow; it reports file, category and count only.
- Release builds are reproducible for `crafthub.exe` (`/Brepro`) and never post-processed.
- Launched apps no longer inherit CraftHub's handles (processes start via `CreateProcessW` with
  handle inheritance disabled).
- Deletion only happens inside folders marked as CraftHub libraries.
