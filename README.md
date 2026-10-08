# CraftHub

**Your creative tools, in one place.**

CraftHub is a free, open-source Windows desktop app that installs, opens, updates, rolls back and
removes the Craft creative apps (PhotoCraft, VectorCraft, FilmCraft and the rest) from their
official GitHub releases.

Source repository: <https://github.com/CraftHubLauncher/CraftHub>

> **Unofficial community application manager. Not affiliated with or endorsed by Storytold or ArtCraft.**
> CraftHub only downloads the apps' own official releases from `github.com/storytold`.

## Supported systems

| OS | Status |
|---|---|
| Windows 10/11, x64 | Supported |
| Windows on ARM, Linux, macOS | Not yet. The code has platform stubs that refuse to install anything. |

## Supported applications

| App | What it is | Installable through CraftHub |
|---|---|---|
| PhotoCraft | Image editing | Yes |
| VectorCraft | Vector illustration | Yes |
| FilmCraft | Video editing | Yes |
| LightCraft | Photo management and RAW development | Yes |
| PDFCraft | PDF editing | Yes |
| EffectCraft | Motion graphics and effects | Yes |
| DesignCraft | Desktop publishing | Yes |
| SoundCraft | Audio editing | Yes (from v0.3.0) |
| WordCraft | Word processing | Yes |
| GridCraft | Spreadsheets | Yes |
| DeckCraft | Presentations | Yes |
| CADCraft | Computer-aided design | Yes |
| ArtCraft | AI creative studio (separate product) | **No.** It ships only as a Windows installer (`setup.exe`/MSI), and CraftHub never runs downloaded installers. It is listed with a link to its releases. |

"Yes" means the app's Windows x64 portable ZIP was audited (layout, executable, published SHA-256).
See [docs/RELEASE_AUDIT.md](docs/RELEASE_AUDIT.md). CraftHub re-checks every download against that
audited layout and refuses anything that doesn't match.

## Installing CraftHub

1. Download `CraftHub_<version>_x64-setup.exe` from this repository's Releases page.
2. Optionally compare its SHA-256 with `SHA256SUMS.txt` on the release page, or verify its build
   provenance with `gh attestation verify <installer> --repo CraftHubLauncher/CraftHub`.
3. Run it. It installs for your user only; no administrator rights are needed.

**SmartScreen warning:** CraftHub builds are not code-signed yet, so Windows shows *"Windows
protected your PC — unknown publisher"*. Choose **More info → Run anyway** only if the file came
from this repository's release page and its SHA-256 matches.

To uninstall CraftHub, use *Settings → Apps → Installed apps*. Apps CraftHub installed are not
removed with it. Remove them in CraftHub first if you want them gone.

## What it does

- **Install** the newest stable release, or a specific version from the app's details panel.
- **Verify** every download before anything is unpacked. The SHA-256 must match GitHub's release
  metadata, and the release's `SHA256SUMS.txt` when one is published. The archive is checked for
  path traversal, links, oversized content and the audited layout.
- **Open** apps directly. CraftHub starts the app's own executable. It never uses a shell and never
  passes extra arguments.
- **Update** one app or all of them. Each new version is unpacked to its own folder and only switched
  on after it has been verified. The previous version is kept for **rollback**. If an app is running,
  CraftHub waits; it never closes it for you.
- **Uninstall** removes only the files CraftHub installed. Your documents and app settings (for
  example `%APPDATA%\Photocraft`) are left alone, and so are unknown files found in an app folder.
- **Recover** after a crash, power loss or a cancelled download. Half-finished work is rolled back
  the next time CraftHub starts.

## Updates

In *Settings → Updates* choose how CraftHub handles new releases:

| Mode | Behaviour |
|---|---|
| Manual | Updates are shown inside CraftHub. Checks happen on start-up (optional) or when you click *Check for updates*. |
| Notify (default) | Also checks in the background at the interval you choose and shows a Windows notification once per new version. Clicking it opens the app in CraftHub. |
| Automatic | Also installs verified updates by itself, but only for apps that are closed. The previous version is kept for rollback. |

You can also pick the release channel (stable or pre-release). CraftHub stays within GitHub's API
limits by caching release data and revalidating it with ETags. When offline it keeps working with
the data it last saw, and installed apps still open.

Closing the window keeps CraftHub in the notification area (configurable). An install or update in
progress is never cut off by closing the window. Use **Quit** in the tray menu to exit; CraftHub
asks before cancelling running work.

**Updating CraftHub itself:** self-update stays switched off until the project's public
repository, its update-signing key and release configuration exist. Once they do, published builds
can update themselves.
The package is verified with a key compiled into CraftHub, not just a GitHub checksum. Builds without
that key (including all local builds) show how to update manually instead.

## Where things are stored

| What | Default location |
|---|---|
| Installed apps | `%LOCALAPPDATA%\Programs\CraftHub\Apps\<app>\<version>_<id>\` |
| Temporary downloads and unpacking | `…\Programs\CraftHub\Downloads` and `…\Staging` |
| CraftHub's database, logs, locks | `%LOCALAPPDATA%\io.github.crafthublauncher.crafthub\` |

You can choose a different folder for **new** installs in *Settings → Install location*. It must be
an empty folder (or one CraftHub created) on a local drive you can write to. System folders,
network drives and links are refused. Apps that are already installed stay where they are.

## Building from source

Requirements: Windows 10/11 x64, Node.js 22.16 or newer, Rust (version pinned in
`rust-toolchain.toml`; rustup installs it automatically), Visual Studio 2022 Build Tools with
"Desktop development with C++", and Microsoft Edge WebView2 (preinstalled on Windows 11).

```powershell
npm ci
npx tauri dev                          # run with hot reload
npm run lint; npm run typecheck; npm test
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                 # unit + integration tests, no network needed
./scripts/build-release.ps1 -- --bundles nsis   # release exe + installer
```

`build-release.ps1` keeps local folder paths out of the binaries at build time (path remapping,
deterministic linking) and then verifies the result with `scripts/privacy-scan.ps1`, which fails on
personal paths, machine details or secrets without printing them. Use it rather than plain `tauri build` for anything you distribute.

There is also a command-line front end over the same engine, for scripting and smoke tests:

```powershell
cargo run -p crafthub-core --bin crafthub-cli -- check
cargo run -p crafthub-core --bin crafthub-cli -- install gridcraft
```

The CLI and the app share data and coordinate through per-app locks, so they never modify the same
app at the same time.

## Current limitations

- Windows x64 only.
- CraftHub and the apps are not Authenticode-signed. A matching SHA-256 proves a file is the one
  attached to the GitHub release, not who built it.
- Self-update needs a release built with an update-signing key. No official release exists yet.
- Apps installed outside CraftHub (for example with an app's own installer) are not detected or adopted.
- ArtCraft cannot be installed (installer-only releases).
- On first start after the public identity change, the old provisional data folder is migrated
  to the final identifier without changing the installed-app library or user settings.

## Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): how the engine, app and UI fit together
- [docs/TRUST_MODEL.md](docs/TRUST_MODEL.md): what download checks prove (and what they don't)
- [docs/SECURITY_AND_UPDATES.md](docs/SECURITY_AND_UPDATES.md): install/update safety rules
- [docs/RELEASE_AUDIT.md](docs/RELEASE_AUDIT.md): evidence behind each installable app
- [docs/RELEASING.md](docs/RELEASING.md): building releases, signing, self-update setup
- [docs/SMOKE_TEST.md](docs/SMOKE_TEST.md): manual end-to-end checklist
- [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), [CHANGELOG.md](CHANGELOG.md), [ASSETS.md](ASSETS.md)

## License

CraftHub is licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at
your option. Third-party icons keep their own licenses; see [ASSETS.md](ASSETS.md). The Craft apps
themselves are separate projects under their own licenses, and CraftHub does not redistribute them.
