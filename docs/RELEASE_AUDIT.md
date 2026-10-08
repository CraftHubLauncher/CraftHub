# Release audit (Milestone 0)

**Audit date:** 2026-10-08
**Method:** `GET https://api.github.com/repos/storytold/{repo}/releases?per_page=10` (unauthenticated REST API),
then download of every Windows x64 portable ZIP of the newest stable release, SHA-256 computed locally and
compared against both the GitHub API `digest` field and the release's own `SHA256SUMS.txt`. ZIP central
directories were listed with .NET `System.IO.Compression.ZipFile` — **nothing was extracted or executed**.

Download URLs (`https://github.com/storytold/{repo}/releases/download/{tag}/{asset}`) answer `302` to
`https://release-assets.githubusercontent.com/github-production-release-asset/...` (observed 2026-10-08).

## Capability matrix (Windows x64)

| App | Repo | Newest stable tag | Published (UTC) | x64 portable asset | API digest | SHA256SUMS | Hash verified | Archive root | Executable | Install enabled |
|---|---|---|---|---|---|---|---|---|---|---|
| PhotoCraft | [storytold/photocraft](https://github.com/storytold/photocraft/releases) | v0.3.0 | 2026-10-07 10:33 | `photocraft-0.3.0-windows-x64-portable.zip` (65,251,740 B) | yes | yes | ✅ `9997f7df…cd47e` | `photocraft-0.3.0-windows-x64-portable/` | `photocraft.exe` | **Yes** |
| VectorCraft | [storytold/vectorcraft](https://github.com/storytold/vectorcraft/releases) | v0.5.0 | 2026-10-08 13:03 | `vectorcraft-0.5.0-windows-x64-portable.zip` (77,469,769 B) | yes | yes | ✅ `98929a4a…2e4c1` | `vectorcraft-0.5.0-windows-x64-portable/` | `vectorcraft.exe` | **Yes** |
| FilmCraft | [storytold/filmcraft](https://github.com/storytold/filmcraft/releases) | v0.2.1 | 2026-10-06 12:18 | `filmcraft-0.2.1-windows-x64-portable.zip` (33,724,271 B) | yes | yes | ✅ `5306251e…57942` | `filmcraft-0.2.1-windows-x64-portable/` | `filmcraft.exe` | **Yes** |
| LightCraft | [storytold/lightcraft](https://github.com/storytold/lightcraft/releases) | v0.2.1 | 2026-10-06 12:10 | `lightcraft-0.2.1-windows-x64-portable.zip` (50,702,290 B) | yes | yes | ✅ `9fae1279…b5208670` | `lightcraft-0.2.1-windows-x64-portable/` | `lightcraft.exe` | **Yes** |
| PDFCraft | [storytold/pdfcraft](https://github.com/storytold/pdfcraft/releases) | v0.2.1 | 2026-10-06 12:37 | `printcraft-0.2.1-windows-x64-portable.zip` (43,436,865 B) | yes | yes | ✅ `7ed1aa95…52872` | `printcraft-0.2.1-windows-x64-portable/` | `printcraft.exe` | **Yes** (asset stem is `printcraft`) |
| EffectCraft | [storytold/effectcraft](https://github.com/storytold/effectcraft/releases) | v0.4.0 | 2026-10-07 09:30 | `effectcraft-0.4.0-windows-x64-portable.zip` (62,338,288 B) | yes | yes | ✅ `b348bb3f…acd2f0` | `effectcraft-0.4.0-windows-x64-portable/` | `effectcraft.exe` | **Yes** |
| DesignCraft | [storytold/designcraft](https://github.com/storytold/designcraft/releases) | v0.2.1 | 2026-10-06 12:15 | `designcraft-0.2.1-windows-x64-portable.zip` (47,696,864 B) | yes | yes | ✅ `307b1281…3ec3f` | `designcraft-0.2.1-windows-x64-portable/` | `designcraft.exe` | **Yes** |
| SoundCraft | [storytold/soundcraft](https://github.com/storytold/soundcraft/releases) | — | — | none (0 releases) | — | — | — | — | — | **No** — no releases published |
| WordCraft | [storytold/wordcraft](https://github.com/storytold/wordcraft/releases) | v0.1.0 | 2026-10-07 22:35 | `wordcraft-0.1.0-windows-x64-portable.zip` (56,167,819 B) | yes | yes | ✅ `2517fe2b…8c201f6` | `wordcraft-0.1.0-windows-x64-portable/` | `wordcraft.exe` | **Yes** |
| GridCraft | [storytold/gridcraft](https://github.com/storytold/gridcraft/releases) | v0.1.0 | 2026-10-08 00:47 | `gridcraft-0.1.0-windows-x64-portable.zip` (15,078,527 B) | yes | yes | ✅ `dd901af3…bdf9` | `gridcraft-0.1.0-windows-x64-portable/` | `gridcraft.exe` | **Yes** |
| DeckCraft | [storytold/deckcraft](https://github.com/storytold/deckcraft/releases) | v0.1.0 | 2026-10-08 00:23 | `deckcraft-0.1.0-windows-x64-portable.zip` (73,460,366 B) | yes | yes | ✅ `8f45a80c…e3e7` | `deckcraft-0.1.0-windows-x64-portable/` | `deckcraft.exe` | **Yes** |
| CADCraft | [storytold/cadcraft](https://github.com/storytold/cadcraft/releases) | v0.1.0 | 2026-10-08 03:07 | `cadcraft-0.1.0-windows-x64-portable.zip` (18,699,650 B) | yes | yes | ✅ `fbac8940…be70` | `cadcraft-0.1.0-windows-x64-portable/` | `cadcraft.exe` | **Yes** |
| ArtCraft (AI studio) | [storytold/artcraft](https://github.com/storytold/artcraft/releases) | artcraft-v0.41.0 | 2026-09-26 11:14 | none — only `ArtCraft_0.41.0_x64-setup.exe` (NSIS) and `ArtCraft_0.41.0_x64_en-US.msi` | yes | no | — | — | — | **No** — installer-only; CraftHub does not execute downloaded installers |

## Archive layout (all 11 audited ZIPs)

Every audited Windows x64 portable ZIP has exactly one top-level directory named after the asset
(`{stem}-{version}-windows-x64-portable/`) containing `{stem}.exe` (GUI), `{stem}-cli.exe` (command line),
licence files and `README.md`. No nested directories, no symlinks. The GUI executable is the launch target.

| App | Entries | Uncompressed bytes | Extra files |
|---|---|---|---|
| PhotoCraft 0.3.0 | 9 | 132,956,598 | `portable.txt`, OFL font licences |
| VectorCraft 0.5.0 | 8 | 168,146,957 | OFL font licences |
| FilmCraft 0.2.1 | 5 | 85,167,785 | — |
| LightCraft 0.2.1 | 5 | 110,476,691 | — |
| PDFCraft (printcraft) 0.2.1 | 5 | 102,858,221 | — |
| EffectCraft 0.4.0 | 5 | 164,434,151 | — |
| DesignCraft 0.2.1 | 5 | 110,616,601 | — |
| WordCraft 0.1.0 | 8 | 111,588,173 | OFL font licences |
| GridCraft 0.1.0 | 8 | 37,764,070 | OFL font licences |
| DeckCraft 0.1.0 | 26 | 149,000,701 | 21 OFL font licences |
| CADCraft 0.1.0 | 5 | 47,692,008 | — |

## Findings that affect the implementation

1. **PhotoCraft `portable.txt` moves user data into the install folder.** Its text (verbatim excerpt):
   *"While this file sits next to photocraft.exe, PhotoCraft keeps its preferences, brush presets,
   crash-recovery autosaves and startup state in the PhotoCraftData folder beside it … Delete this file to
   use the normal per-user settings folder (%APPDATA%\Photocraft) instead."*
   Because CraftHub installs into version-specific folders and replaces them on update, CraftHub removes
   `portable.txt` from the staged copy (declared per app in the catalog as `stripFiles`) so PhotoCraft
   keeps user data in `%APPDATA%\Photocraft`, outside anything CraftHub manages. As defence in depth,
   all removals are manifest-based: CraftHub deletes only files it extracted and never deletes unknown
   files (they are left in place and reported).
2. **Pre-release flag is unreliable.** PhotoCraft `v0.1.1-rc.4` and `v0.1.1-rc.5` have `prerelease: false`
   in the API. CraftHub treats a release as pre-release if the API flag is set **or** the parsed semver
   version has a pre-release component.
3. **Tag formats differ.** Craft apps use `vX.Y.Z`; ArtCraft uses `artcraft-vX.Y.Z`. Version parsing
   strips a known prefix per app and compares parsed semver (never strings).
4. **PDFCraft asset stem is `printcraft`.** The catalog stores the asset stem explicitly; the resolver
   never derives asset names from the repo slug.
5. **Every asset carries a GitHub `digest`** (`sha256:…`) and each release has a `SHA256SUMS.txt`.
   Both agreed with locally computed hashes for all 11 ZIPs. The digest is computed by GitHub at upload
   time; it proves the bytes match what was uploaded to the release, **not** who built them. Upstream
   binaries are not Authenticode-signed as far as this audit determined (not verified per binary).
6. Older releases of PhotoCraft, VectorCraft, FilmCraft and EffectCraft use the same asset naming and
   all carry digests, so installing a specific older version uses the same resolver.

## Checksum policy (decision)

- Required: GitHub API `digest` with `sha256:` prefix for the selected asset. The download is hashed
  while streaming and compared before anything is extracted.
- If the release also publishes `SHA256SUMS.txt`, CraftHub downloads it (size-capped) and requires the
  entry for the asset to agree. A mismatch blocks install.
- If no digest is available, installation is **blocked** with an explanation. There is no "install
  anyway" override in this version.
- UI wording: "SHA-256 matches GitHub release metadata" — never "signed" or "authenticated publisher".

## Re-audit procedure

When a catalog entry changes or upstream changes its packaging, rerun the API query, download the ZIP,
compare hashes and list entries, then update this file and `catalog/apps.json` (`audit` block).
CraftHub additionally re-validates the archive layout on every install and refuses archives that
deviate from the audited layout (missing root folder, missing executable, unexpected nesting).

---

## Re-audit — 2026-10-08 (afternoon)

Upstream published new releases the same day. Method: Releases API for all 13 repos, then each newest
stable Windows x64 portable ZIP was inspected by reading **only its central directory** via HTTP range
requests (3 requests per asset; the archive is not downloaded). SoundCraft, being newly enabled, was also
downloaded in full and hash-checked.

| App | Newest stable | Asset | Root folder OK | Executable | Portable marker | Digest | Result |
|---|---|---|---|---|---|---|---|
| PhotoCraft | v0.5.0 | `photocraft-0.5.0-windows-x64-portable.zip` | yes | `photocraft.exe` | `portable.txt` (stripped) | yes | installable |
| VectorCraft | v0.6.0 | `vectorcraft-0.6.0-…` | yes | `vectorcraft.exe` | none | yes | installable |
| FilmCraft | v0.4.0 | `filmcraft-0.4.0-…` | yes | `filmcraft.exe` | none | yes | installable |
| LightCraft | v0.4.0 | `lightcraft-0.4.0-…` | yes | `lightcraft.exe` | none | yes | installable |
| PDFCraft | v0.4.0 | **`pdfcraft-0.4.0-…`** (renamed) | yes | **`pdfcraft.exe`** | none | yes | installable via new variant |
| EffectCraft | v0.6.0 | `effectcraft-0.6.0-…` | yes | `effectcraft.exe` | none | yes | installable |
| DesignCraft | v0.4.0 | `designcraft-0.4.0-…` | yes | `designcraft.exe` | none | yes | installable |
| SoundCraft | v0.3.0 | `soundcraft-0.3.0-windows-x64-portable.zip` (13,612,929 B) | yes | `soundcraft.exe` | none | yes | **newly enabled** |
| WordCraft | v0.3.0 | `wordcraft-0.3.0-…` | yes | `wordcraft.exe` | none | yes | installable |
| GridCraft | v0.3.0 | `gridcraft-0.3.0-…` | yes | `gridcraft.exe` | none | yes | installable (installed and launched) |
| DeckCraft | v0.3.0 | `deckcraft-0.3.0-…` | yes | `deckcraft.exe` | none | yes | installable |
| CADCraft | v0.3.0 | `cadcraft-0.3.0-…` | yes | `cadcraft.exe` | none | yes | installable (installed and launched) |
| ArtCraft | artcraft-v0.41.0 | `ArtCraft_0.41.0_x64-setup.exe`, `.msi`, macOS `.dmg`/`.app.tar.gz` | — | — | — | — | still **unavailable** (installer only) |

SoundCraft 0.3.0 full check: SHA-256 `36429165e703f3dfbdbc82dc517235422e420685852424bfc4bf112db6ce679f` matches the
API digest and `SHA256SUMS.txt`. Entries: `ATTRIBUTION.md`, `LICENSE-APACHE`, `LICENSE-MIT`, `NOTICE`, `README.md`,
`soundcraft-cli.exe`, `soundcraft.exe` under `soundcraft-0.3.0-windows-x64-portable/`.

### Catalog changes (schema 3)

- `windowsX64.variants` replaces the single `assetStem`/`executable`. A release must match exactly one
  audited variant; zero or several matches make it not installable. PDFCraft lists `pdfcraft` (≥ 0.4.0) and
  `printcraft` (≤ 0.2.1). Each install records the executable of the variant it used.
- SoundCraft enabled. ArtCraft unchanged.

### Upstream icons and trademarks

- Each Craft repo's `assets/app-icon/LICENSE.txt` (where present) declares the icon original artwork under
  MIT OR Apache-2.0. Present for PhotoCraft, VectorCraft, FilmCraft, LightCraft, PDFCraft, EffectCraft,
  DesignCraft, GridCraft; absent for SoundCraft, WordCraft, DeckCraft, CADCraft. See `ASSETS.md`.
- The ArtCraft name/wordmark/mark (`docs/brand/` upstream) are trademarks, not open source, licensed only for
  use within the official apps. CraftHub does not use them.

### Other observation

An ArtCraft installation made outside CraftHub exists on the test machine (`%LOCALAPPDATA%\ArtCraft`).
CraftHub neither detects nor touches external installs (adoption is deferred).
