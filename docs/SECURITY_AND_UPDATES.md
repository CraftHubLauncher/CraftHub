# Update engine — safety requirements

> Implementation status: all rules below are implemented (see `docs/ARCHITECTURE.md`), with these
> documented exceptions: binaries are not Authenticode-signed; releases without a digest are
> blocked rather than offered with consent; installs made outside CraftHub are not detected.

1. Enumerate official GitHub release metadata for allowlisted `storytold/*` repos; verify exact app ID mapping, version, asset format, OS and arch. Do not pick an asset using naive substring matching.
2. Select only validated `*-windows-x64-portable.zip` assets *after* auditing real release artifacts and internal executable layout; other formats require explicit installer adapters.
3. Download via HTTPS with domain/redirect validation, strict timeout, max size, streaming progress, cache bounded, cancellation and free-space checks. Do not log auth tokens.
4. Validate GitHub API digest metadata when available (e.g. `digest: sha256:...`). Compare to computed SHA-256 in constant-time-style crypto library comparison when appropriate. Distinguish transport integrity from publisher authenticity; unsigned upstream assets remain unsigned. If digest unavailable, do not invent trust; implement explicit block or documented user-consent policy.
5. Extract into a unique CraftHub-owned staging directory; reject absolute/`..`/drive-prefixed paths, symlinks, reparse points, device files, excessive entry counts, compression bombs, file collisions, and archive escapes. Limit total expanded bytes. Confirm exact executable through an audited manifest, not by launching a random EXE.
6. Ensure target application is not running; show close-app prompt. NEVER forcibly kill the app or discard unsaved projects.
7. Install to version-specific managed folder; validate, then activate by switching a small managed pointer/state record. Retain previous version as rollback target; on failure restore registry and launch target. If true atomic switching across Windows file systems is impossible, use journalled recovery and honest state transitions.
8. After successful update, clean old versions according to retention policy. Never remove shared user preference directories; never delete outside canonical CraftHub install root. Deletion requires explicit matching managed registry record.
9. On power loss/crash, use persisted operation journal to identify incomplete stages and recover/clean safely before presenting success.
10. Changes to CraftHub itself use a *separate*, cryptographically signed updater and must not replace the running binary directly.

## Security test scenarios
ZIP slip (`../`), absolute paths, Windows path weirdness, symlink escape, oversized archive, wrong hash, truncated network response, release asset swapped, redirects to unknown origin, disk full, app in use, interrupted rename, stale SQLite rows, malicious app IDs, deletion path injection, API 403/rate-limits, no compatible release, untrusted MIME/file extension.

## Security policy
Never run PowerShell or arbitrary post-install hooks fetched from GitHub; no admin elevation for standard installs. Provide conspicuous warning about unofficial launcher and binary origin. Validate Tauri IPC inputs. No telemetry by default.

Per-application installation parents are validated before use: local-drive only, no UNC/device
paths or reparse points, no protected/application-data folders, and no non-empty unmanaged
folders. Updates use the recorded parent and never silently follow a changed global default.
Desktop shortcuts are written through the Windows ShellLink COM API. CraftHub records the exact
path and verifies an embedded ownership marker before updating or removing a shortcut; automatic
updates only retarget an existing managed shortcut.
