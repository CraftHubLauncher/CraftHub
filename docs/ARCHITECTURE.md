# Architecture (as implemented)

## Workspace

```
catalog/apps.json                  curated catalog (schema v2), compiled into the binary
crates/crafthub-core/              all side effects; no Tauri dependency
  src/catalog.rs                   validated loader (schema 3); repo-owner allowlist; audited asset variants
  src/releases/github.rs           GitHub REST client, ETag cache, rate-limit/offline handling
  src/releases/version.rs          tag → semver, channel rules (API flag OR semver pre-release)
  src/releases/resolver.rs         exact asset-name + canonical-URL resolver, digest parsing
  src/net.rs                       HTTPS host allowlist enforced on every redirect hop
  src/installer/download.rs        streaming download, exact size, SHA-256, cancellation
  src/installer/extract.rs         validate-then-extract ZIP; PE x64 header check
  src/registry.rs                  SQLite (WAL, schema 3): installs (+per-app library and shortcut ownership), manifests, op journal, cache, settings, kv, events
  src/locks.rs                     per-app cross-process file locks (LockFileEx via std File::try_lock)
  src/paths.rs                     managed folders; the only deletion code (manifest-based)
  src/platform/{windows,unsupported}.rs  process detection, free space, reparse points, spawn
  src/platform/mod.rs              Windows ShellLink shortcuts and Desktop known-folder handling
  src/engine.rs                    orchestration: check, install/update (transactional), rollback,
                                   uninstall, launch, update-all, crash recovery
  src/bin/crafthub-cli.rs          headless front-end for smoke tests/scripting
  tests/engine.rs, tests/extract.rs  integration tests (local mock GitHub, ZIP fixtures)
src-tauri/                         Tauri 2 shell
  src/commands.rs                  typed IPC commands (folder picker runs in Rust)
  src/desktop.rs                   tray, close-to-tray, notifications, background checks, auto-update
  src/selfupdate.rs                CraftHub self-update (Tauri updater; compile-time key + endpoint)
  build.rs                         declares every command → generated permissions
  capabilities/default.json        grants exactly those commands + event listen; nothing else
src/                               React 19 + TypeScript UI (Vite)
  api.ts / types.ts                typed IPC contract mirroring Rust serde types
  store.tsx                        app state, progress events, actions (API injectable for tests)
  components/, views/              sidebar, top bar, cards, drawer, dialogs, settings
```

## Boundaries

The web view can call only the commands listed in `src-tauri/build.rs` (enforced by the
capability file). It has no filesystem, shell, HTTP or URL-opening permissions. Every URL
CraftHub opens or downloads, and every path it touches, is derived in Rust from the catalog,
the GitHub API response for an allowlisted repo, or the registry — never from UI input.
App ids and versions from IPC are validated before use.

## Install / update transaction

1. Fresh release list (ETag revalidated; stale cache allowed if GitHub is unreachable).
2. Exact asset `{assetStem}-{version}-windows-x64-portable.zip`; download URL must equal
   `https://github.com/{repo}/releases/download/{tag}/{asset}`; GitHub `digest` required.
3. Update only: refuse if any process image lives under `Apps\{app}\` (no force-closing).
4. Free-space check; stream to `Downloads\{op}.zip.part` while hashing; enforce exact size;
   redirect hops checked against the HTTPS host allowlist.
5. Constant-time SHA-256 comparison with the API digest; cross-check `SHA256SUMS.txt` if published.
6. Validate the whole archive (names, symlinks, collisions, sizes, ratios, layout) then
   extract into `Staging\{op}\content`, stripping the audited root folder.
7. Remove audited `stripFiles`; verify the executable exists and is an x86-64 PE image.
8. Re-check the app is not running. Journal state `activating` + write the file manifest.
9. `rename` staging → `Apps\{app}\{version}_{op8}` (same volume; unique name, never collides).
10. One SQLite transaction makes it active and demotes the old active version to "previous".
11. Post-commit: delete the version that fell out of retention (manifest-based), clean temp.

Any failure before step 10 removes the new folder via its manifest and leaves the registry
untouched, so the existing version keeps working. On startup, recovery reads unfinished
journal rows, removes half-activated folders, sweeps orphan manifests and clears temp items.

## State and concurrency

- One operation per app: an in-process busy set plus a **cross-process file lock** per app
  (`<data dir>/locks/<app>.lock`, released by the OS if the holder dies). The GUI, the CLI or a
  second engine get a `Busy` error instead of racing. Max two parallel downloads; Update All is
  sequential. A single-instance plugin prevents two GUI processes.
- Crash recovery runs at every engine start but only repairs apps whose lock it can take; temp
  items are removed only if their operation is unknown or its app lock is free, so a CLI starting
  next to a busy GUI never disturbs it.
- Views are rebuilt from SQLite + cached release JSON on demand, so startup and offline
  use need no network.

## Install libraries

New installs show an installation dialog with the application, version, selected parent and the
final app-specific folder. The parent defaults to `%LOCALAPPDATA%\Programs\CraftHub`, and the
native Rust folder picker can select another safe local folder. Each install record and manifest
stores its library (`NULL` = default), so changing the global root never moves existing installs;
updates, rollback and uninstall stay in the app's recorded library. Roots are validated (local
drive, no links/UNC/system folders, empty or already a library, writable) and marked with
`.crafthub-library`; nothing is deleted in a root without that marker.

## Desktop integration

- Tray icon with Open / Check for updates / Quit. Closing the window hides to the tray when
  enabled, and always while an install/update runs. Quit with work in progress asks the UI to
  confirm, then cancels (each operation rolls back) before exiting.
- Background loop: optional startup check; in Notify/Automatic mode, checks every N hours.
  Automatic mode runs Update All (skips running apps). Native WinRT toasts announce each new
  version once (`kv` key `notified:<app>`); clicking one emits a navigate event that opens the
  app's details.
- Apps are launched with `CreateProcessW` and `bInheritHandles = FALSE`.
- Optional desktop shortcuts are real Windows ShellLink `.lnk` files. They resolve the redirected
  Desktop through `FOLDERID_Desktop`, point to the verified installed executable, use its icon and
  working directory, and carry a CraftHub ownership marker. Updates preserve and retarget managed
  shortcuts; uninstall removes only a shortcut whose ownership marker matches the app. Automatic
  updates never create a new shortcut.

## Platform adapters

`platform/` contains the OS primitives. Windows is implemented; other targets compile to
`unsupported.rs`, which returns explicit `Unsupported` errors for Craft application process,
free-space, launch and folder operations. Linux packages therefore provide the CraftHub desktop
UI but do not claim Craft application installation/update/rollback/uninstall support. Catalog
entries carry a `windowsX64` adapter only; enabling Linux application management requires
separately audited catalog fields and installer paths.

## Self-update

Separate from the app engine. Uses the Tauri updater plugin, registered only when the build has
`CRAFTHUB_UPDATER_PUBKEY` and a valid `CRAFTHUB_UPDATE_ENDPOINT`
(`https://github.com/CraftHubLauncher/CraftHub/releases/latest/download/latest.json`) at compile time. The
package's minisign signature is verified against the embedded public key before CraftHub's own
installer runs, so authenticity does not rest on a GitHub checksum. Installing needs user approval
and is refused while operations are running. Builds without the key report the reason in Settings
and never download anything.
