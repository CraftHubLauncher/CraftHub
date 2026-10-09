# Releasing CraftHub

## Build locally

```powershell
npm ci
./scripts/build-release.ps1 -- --bundles nsis   # release binary + NSIS installer (per-user, no admin)
```

## Linux x86_64 packages

The Linux release job is a native Tauri build and produces both required formats:

```bash
npm ci
bash ./scripts/build-release-linux.sh
```

The build command only generates and checks that both package files exist. Package-aware privacy
validation is a separate, repeatable step and does not recompile CraftHub:

```powershell
pwsh -NoProfile -File ./scripts/scan-linux-packages.ps1 -AppImagePath ./target/release/bundle/appimage/CraftHub_<version>_amd64.AppImage -DebPath ./target/release/bundle/deb/crafthub_<version>_amd64.deb
```

The host needs `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`,
`patchelf`, `xvfb`, and the standard C/C++ build tools. CI validates package metadata, launches
both packages under Xvfb, runs the Linux release privacy scan, and uploads the AppImage and Debian
package as separate artifacts. The AppImage is validated on Ubuntu; CachyOS/Arch verification is
still required manually because WebKitGTK and desktop integration vary by distribution.

Linux packages use CraftHub's existing product name, identifier, version and Tauri icon set.
Self-update remains disabled. Craft application install/update/rollback/uninstall remains
Windows-only until Linux-specific catalog and filesystem behavior is audited.

## Release workflow integrity

- Third-party GitHub Actions are pinned to full commit SHAs whose commits GitHub shows as
  verified. The trailing comment names the release, and the commit is the one the action's major
  tag pointed to when pinned. Update pins deliberately: resolve the new tag to its commit, check it,
  and change the SHA and comment together.
- The release job uses no build cache, runs the same checks as CI, enforces `Cargo.lock`
  (`--locked`) and `package-lock.json` (`npm ci`), and requires the tag to equal the version in
  `src-tauri/tauri.conf.json`, `package.json` and `Cargo.toml`.
- `SHA256SUMS.txt` covers every published file. `actions/attest-build-provenance` records a
  Sigstore provenance attestation for the installer and the checksum file. After upload, the
  assets are downloaded again and compared with the checksums.
- The installer embeds Microsoft's WebView2 bootstrapper (`webviewInstallMode:
  embedBootstrapper`) instead of downloading it during installation, so nothing is fetched and run
  at install time, and the bootstrapper is covered by the checksum and attestation.

## Privacy and reproducibility

Prevention happens at build time; release binaries are never patched or stripped afterwards, so
they stay normal linker output that can be Authenticode-signed:

- `--remap-path-prefix` maps the Cargo home, rustup home and repository root to `/cargo`,
  `/rustup` and `/crafthub` (stable Cargo has no `trim-paths` yet).
- TLS uses rustls with `ring`; aws-lc (whose C sources embed absolute paths) is not built.
- `/Brepro` gives deterministic PE timestamps; `strip = true`, no debug info, PDB referenced by
  file name only, no frontend source maps.

`scripts/privacy-scan.ps1` then **verifies** the output (read-only):

| Mode | Where it runs | What it checks |
|---|---|---|
| `-SelfTest` | CI, release workflow | Plants synthetic samples of every category and fails unless all are detected and none is printed |
| `-Mode Repo` | CI, release workflow | Every committable text file, plus binary assets: images must carry no metadata (PNG text/EXIF/time/unknown chunks, also inside ICO/ICNS; JPEG/WebP EXIF; XMP), decoded metadata and ICC profiles are pattern-checked, and executables, archives, databases and key stores are rejected |
| `-Mode Release` | end of `build-release.ps1` (local, CI `release-build` job, release workflow) | `crafthub.exe`, the installer and their version-info fields |
| `-Mode LinuxRelease` | end of `build-release-linux.sh` (CI and release workflow) | the AppImage and Debian package |

Categories: this machine's user name, computer name, profile path, repository path and Git
e-mail; any `C:\Users\<name>\` path; Cargo/rustup home paths; Unix home paths; absolute PDB
paths; GitHub tokens; PEM private keys; minisign/rsign secret keys; AWS keys; Slack tokens;
e-mail addresses (repo: except example/no-reply domains; binaries: except the third-party credits
in `scripts/privacy-allowlist.txt`; the installer's compressed payload is not e-mail-scanned, but
its uncompressed `crafthub.exe` is). Output names only the file, the category and a count, never
the matched text. Deliberate test fixtures can opt out per line with `privacy-scan: allow`.

Outputs:

- `target\release\crafthub.exe` — the app (runs without installing)
- `target\release\bundle\nsis\CraftHub_<version>_x64-setup.exe` — per-user installer

Always invoke `scripts/build-release.ps1` for distributable builds. A direct `npx tauri build`
does not install the release script's `CARGO_ENCODED_RUSTFLAGS`, so rustc can retain Cargo
registry/rustup source paths in dependency panic metadata. The hardened script remaps those
paths at compile time and runs the release privacy scan before it returns; no binary patching or
scanner bypass is permitted.

## GitHub release

Pushing a tag `vX.Y.Z` (matching `version` in `src-tauri/tauri.conf.json`) runs
`.github/workflows/release.yml`: tests, privacy-safe build, `SHA256SUMS.txt`, and a **draft**
release that a maintainer reviews and publishes.

## Code signing (Authenticode) — not in place

CraftHub binaries are **unsigned**. Windows SmartScreen will show "Windows protected your PC /
Unknown publisher" for the installer. Do not describe releases as signed.

To sign in the future:

1. Obtain an Authenticode certificate (OV/EV, or Azure Trusted Signing).
2. Configure `bundle.windows.signCommand` (or `certificateThumbprint`) in
   `src-tauri/tauri.conf.json`; keep keys in CI secrets or an HSM, never in the repo.
3. Sign both `crafthub.exe` and the installer; publish SHA-256 sums with the release.

## Enabling CraftHub self-update (one-time setup)

1. Generate an update-signing key pair **offline**: `npx tauri signer generate -w crafthub-updater.key`.
   Keep the private key and its password out of the repository and back them up securely.
2. In the GitHub repository settings add:
   - secret `TAURI_SIGNING_PRIVATE_KEY` (contents of the private key file)
   - secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (if you set one)
   - variable `CRAFTHUB_UPDATER_PUBKEY` (the public key)
3. Set `PRODUCTION_REPOSITORY` in `src-tauri/src/selfupdate.rs` to
   `Some("CraftHubLauncher/CraftHub")` in a reviewed change only after signed production
   releases exist (the release workflow refuses to sign updates otherwise).
4. Tag a release. The workflow builds with `src-tauri/tauri.updater.conf.json`
   (`createUpdaterArtifacts`), embeds the public key and the endpoint
   `https://github.com/<owner>/<repo>/releases/latest/download/latest.json`, and uploads the
   installer, its `.sig` file and `latest.json`.
5. Only builds produced this way can update themselves. Losing the private key means users must
   reinstall manually once; rotating it requires shipping the new public key in a release signed
   with the old one.

Self-update is gated in code (`src-tauri/src/selfupdate.rs`, unit-tested). It stays **off**
unless all of these hold for the build: `PRODUCTION_REPOSITORY` in that file is set to the real
`CraftHubLauncher/CraftHub` repository (it is `None` today); the bundle identifier is
`io.github.crafthublauncher.crafthub` for that
owner; a minisign *public* key is embedded (a pasted private key is rejected); and the endpoint is
exactly `https://github.com/<owner>/<repo>/releases/latest/download/latest.json` for that same
repository (owner **and** repository name). The updater plugin is not even registered otherwise, and the release workflow refuses
to publish update artifacts if the identifier doesn't match the repository owner. Local and
unsigned builds explain the reason in Settings and never download updates.

## Provenance of managed apps

CraftHub downloads Craft apps from `github.com/storytold/*` releases and verifies the SHA-256
digest GitHub records for each asset. That proves the file matches what was uploaded to the
release. It does not prove who built it; upstream binaries carry no signature CraftHub checks.
