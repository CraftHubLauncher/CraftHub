# Contributing to CraftHub

Thanks for helping! CraftHub installs software on people's computers, so correctness and safety
come before features.

## Ground rules

- CraftHub is an **unofficial** project. Don't add anything that suggests affiliation with or
  endorsement by Storytold or ArtCraft. Don't add upstream logos or artwork unless their license
  allows it, and record them in [ASSETS.md](ASSETS.md).
- Never weaken the install pipeline: HTTPS host allowlist, `storytold/*` repo allowlist, exact
  asset-name matching, mandatory SHA-256, validate-before-extract, manifest-based deletion, per-app
  locks. If a change touches these, explain why in the PR and add tests.
- CraftHub must never run downloaded installers or scripts, close a user's running app, or elevate.
- Every button must do something real. If something isn't possible, disable it and say why.
- Don't commit personal data (names, e-mail addresses, local paths), secrets or signing keys.

## Development setup

See *Building from source* in the [README](README.md). The repository layout:

| Path | Contents |
|---|---|
| `crates/crafthub-core/` | Engine: catalog, GitHub client, resolver, download, ZIP extraction, registry, recovery, CLI |
| `src-tauri/` | Desktop shell: IPC commands, tray, notifications, background checks, self-update |
| `src/` | React + TypeScript UI |
| `catalog/apps.json` | Curated app list and audited Windows adapters (compiled into the app) |
| `scripts/` | Release build, privacy check, upstream release/icon audit helpers |
| `docs/` | Architecture, security design, release audit, releasing, smoke test |

## Before opening a pull request

Run everything CI runs:

```powershell
npm run format:check; npm run lint; npm run typecheck; npm test; npm run build
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Also run `./scripts/privacy-scan.ps1 -Mode Repo` (CI runs it too). It reports only file,
category and count; fix findings rather than allowlisting them unless they are deliberate test
fixtures (mark the line with `privacy-scan: allow`).

Tests must not need the network. Use the local mock GitHub server and ZIP fixtures in
`crates/crafthub-core/tests/common/` for engine behaviour.

## Adding or changing an app in the catalog

1. Run `python -I scripts/audit_releases.py <app>`. It reads only the ZIP's central directory
   through HTTP range requests and reports the root folder, executable, portable-mode markers and
   digest. For a newly enabled app, also download the ZIP and compare its SHA-256 with the release
   digest and `SHA256SUMS.txt`.
2. Record the findings, with date and versions, in [docs/RELEASE_AUDIT.md](docs/RELEASE_AUDIT.md).
3. Update `catalog/apps.json`. Each `windowsX64.variants` entry is an exact asset-name scheme and
   executable. Add `stripFiles` if the archive contains a file that would put user data inside
   the install folder (as PhotoCraft's `portable.txt` does).
4. Never mark an app installable because "it probably works". No audit, no install button.

## Commit and PR conventions

- Small, focused PRs with a clear description of user-visible behaviour.
- Update `CHANGELOG.md` under *Unreleased*.
- Configure Git to use an address you are happy to publish, for example GitHub's no-reply
  address: `git config user.email "<id>+<username>@users.noreply.github.com"`.

By contributing you agree that your contributions are licensed under MIT OR Apache-2.0, as
described in [LICENSE](LICENSE).
