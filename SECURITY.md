# Security policy

CraftHub downloads and installs software, so security reports are very welcome.

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Use GitHub's private vulnerability
reporting on this repository instead (*Security → Report a vulnerability*). Include the CraftHub
version, Windows version, steps to reproduce and the impact you expect.

You should get an acknowledgement within a few days. Fixes are released as soon as practical, and
reporters are credited unless they ask not to be.

## Supported versions

Only the latest release receives security fixes during the 0.x series.

## Scope

In scope:

- Anything that makes CraftHub download from an unexpected origin, accept a file whose SHA-256
  doesn't match the release metadata, or install an archive that escapes its folder.
- Deleting or modifying files outside CraftHub's managed folders, or user data inside them.
- Bypassing the per-app locks, the running-app check, or the update rollback.
- Using the web view or IPC to reach the filesystem, shell, network or URLs beyond what the
  command list allows.
- Self-update accepting a package not signed with the configured key.

Out of scope:

- Vulnerabilities in the Craft apps themselves. Report those to their own repositories.
- The fact that CraftHub and upstream builds are not Authenticode-signed (a documented limitation).
- Attacks that require an attacker who already controls the user's account or the
  `github.com/storytold` repositories.

## What CraftHub does and doesn't guarantee

A matching SHA-256 proves a download is byte-for-byte the file attached to the GitHub release. It
does **not** prove who built that file. CraftHub never labels upstream binaries as signed or
authenticated. The full trust model is in [docs/TRUST_MODEL.md](docs/TRUST_MODEL.md); the design is described in
[docs/SECURITY_AND_UPDATES.md](docs/SECURITY_AND_UPDATES.md)
and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
