# Download trust model

This page explains what CraftHub checks when it downloads an app, and what those checks do and
do not prove. It covers the Craft apps that CraftHub installs and CraftHub's own releases.

## Short version

- CraftHub installs only files attached to official GitHub releases of the allowlisted
  `github.com/storytold/*` repositories, fetched over HTTPS from GitHub's own hosts.
- Before anything is unpacked, the file's SHA-256 must equal the digest GitHub publishes for
  that release asset (and the release's `SHA256SUMS.txt`, when present).
- **That proves the file is the one attached to the release, byte for byte. It does not prove
  who built it.** CraftHub trusts whoever can publish releases in those repositories.
- Upstream Craft binaries are not Authenticode-signed, and CraftHub does not verify any
  publisher signature for them. CraftHub never calls them "signed" or "authenticated".

## What each check protects against

| Check | Protects against | Does not protect against |
|---|---|---|
| Repository allowlist (`storytold/*` in the compiled-in catalog) | Installing from look-alike or attacker repositories | A compromise of the allowlisted repositories themselves |
| HTTPS, host allowlist on every redirect (`api.github.com`, `github.com`, GitHub's asset CDN hosts) | Network attackers, DNS/redirect tricks, downloads from other hosts | A compromised GitHub account or GitHub itself |
| Exact asset name and canonical download URL for the audited variant | Picking the wrong file, look-alike names, assets of other repos | A malicious file uploaded under the expected name by someone with release access |
| Exact size and streaming SHA-256 vs GitHub's asset `digest` (constant-time compare) | Corrupted, truncated, swapped or tampered downloads, CDN or proxy errors | Malicious content that GitHub recorded as the release asset |
| Cross-check with the release's `SHA256SUMS.txt` | Inconsistency between GitHub's digest and the publisher's own list | Both being written by the same compromised publisher |
| Validate-before-extract ZIP rules, audited layout, x86-64 PE check | Path traversal, links, archive bombs, unexpected or misplaced executables | Malicious code inside a well-formed executable |
| No installers or scripts are run; no elevation | Code running with admin rights or during install | Code running when the user opens the app |

Where the digest comes from: GitHub computes the `digest` of each release asset when it is
uploaded and serves it from its API. It is independent of the download path, which is why it
catches transport and storage problems. But anyone who can upload an asset also determines its
digest, so it is an **integrity** check, not a **publisher authentication** check.

## What would change the picture

- **Upstream signatures:** if the Craft apps start publishing Authenticode signatures, minisign
  or Sigstore signatures, or GitHub artifact attestations, CraftHub can verify them against keys or
  identities pinned in the catalog. That would authenticate the publisher. It is not implemented,
  because no such signatures exist today.
- **Repository compromise:** if a `storytold/*` repository or a maintainer account were compromised,
  CraftHub would install the attacker's release like any other. Mitigations: CraftHub keeps the
  previous version for rollback; the catalog is compiled in, so redirecting CraftHub to another
  repository needs a new CraftHub release; and the audited layout limits what an archive may contain.

## CraftHub's own releases

| Artifact | Integrity | Authenticity |
|---|---|---|
| Installer on the GitHub release page | `SHA256SUMS.txt`, GitHub asset digests, and a **build-provenance attestation** (Sigstore) tying the file to the release workflow and commit: `gh attestation verify <installer> --repo CraftHubLauncher/CraftHub` | No Authenticode signature yet, so SmartScreen warns |
| Self-update (when enabled) | Tauri updater downloads `latest.json` and the installer from the configured repository | **minisign signature** checked against a public key compiled into CraftHub, so it does not rely on GitHub checksums. Disabled until the public repository, key and configuration exist (`src-tauri/src/selfupdate.rs`) |

## For reviewers

- Catalog and repository allowlist: `crates/crafthub-core/src/catalog.rs`, `catalog/apps.json`
- Origin policy: `crates/crafthub-core/src/net.rs`
- Asset resolution: `crates/crafthub-core/src/releases/resolver.rs`
- Download and hash: `crates/crafthub-core/src/installer/download.rs`; comparison and checksum
  cross-check in `crates/crafthub-core/src/engine.rs`
- Archive validation: `crates/crafthub-core/src/installer/extract.rs`
- Evidence per app: `docs/RELEASE_AUDIT.md`
