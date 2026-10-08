# Third-party assets

CraftHub's own code and artwork (the official CraftHub source image in
`src/assets/branding/craftHubIcon.png`, its generated Tauri platform assets in
`src-tauri/icons/`, and the in-app branding reference in `src/components/Chrome.tsx`)
are original to this project. This file lists every third-party asset shipped in the repository or
the built application, where it came from and under which terms it is used.

## Craft app icons

Upstream ships each app's icon under `assets/app-icon/` with its own `LICENSE.txt`. CraftHub bundles an
icon **only** when that file states the icon is original artwork licensed under MIT OR Apache-2.0 (checked
2026-10-08). CraftHub uses them under the MIT option; the upstream MIT text (with its copyright line) and
the icon `LICENSE.txt` are kept next to each image. The files are unmodified copies pinned to the commit
shown.

| App | File | Source (pinned commit) | License | SHA-256 |
|---|---|---|---|---|
| PhotoCraft | `src/assets/app-icons/photocraft.png` (128x128) | [storytold/photocraft@e5e3e397523b](https://github.com/storytold/photocraft/blob/e5e3e397523b4db6a016dd19526e4c68514fcc72/assets/app-icon/hicolor/128x128/apps/ai.storyteller.photocraft.png) | MIT OR Apache-2.0 (`photocraft.LICENSE.txt`, `photocraft.LICENSE-MIT.txt`) | `f1afe9ccdb125a34089babecb6c1533f3fbf41ddffe81e0856f158c5b5fb9389` |
| VectorCraft | `src/assets/app-icons/vectorcraft.png` (128x128) | [storytold/vectorcraft@99a5318fe44b](https://github.com/storytold/vectorcraft/blob/99a5318fe44b610a8cac0f1f27fa8fe6994623f4/assets/app-icon/hicolor/128x128/apps/ai.storyteller.vectorcraft.png) | MIT OR Apache-2.0 (`vectorcraft.LICENSE.txt`, `vectorcraft.LICENSE-MIT.txt`) | `47f2061e3683ba8e7241c4783ca02207d29dfef0a7c94cd0eb8ea0a88dfc0b1e` |
| FilmCraft | `src/assets/app-icons/filmcraft.png` (128x128) | [storytold/filmcraft@523185244336](https://github.com/storytold/filmcraft/blob/5231852443363f001c3f6b396dd9b1e6461ae2be/assets/app-icon/hicolor/128x128/apps/ai.storyteller.filmcraft.png) | MIT OR Apache-2.0 (`filmcraft.LICENSE.txt`, `filmcraft.LICENSE-MIT.txt`) | `7ca8adb57a34d1f4bff8c7e976ac5fe73e69a89b4268d15b3b861c7e51c45a74` |
| LightCraft | `src/assets/app-icons/lightcraft.png` (128x128) | [storytold/lightcraft@2472021091a2](https://github.com/storytold/lightcraft/blob/2472021091a28eb93a05947cfc9b941d3911543a/assets/app-icon/hicolor/128x128/apps/ai.storyteller.lightcraft.png) | MIT OR Apache-2.0 (`lightcraft.LICENSE.txt`, `lightcraft.LICENSE-MIT.txt`) | `a1fc54a714b769fa627eb44b0dcc1230e5fc3643890fd3504811b4c05c4a913a` |
| PDFCraft | `src/assets/app-icons/pdfcraft.png` (128x128) | [storytold/pdfcraft@22752dc91577](https://github.com/storytold/pdfcraft/blob/22752dc9157706e55ead60b89a857a9ce03c8fc0/assets/app-icon/hicolor/128x128/apps/ai.storyteller.pdfcraft.png) | MIT OR Apache-2.0 (`pdfcraft.LICENSE.txt`, `pdfcraft.LICENSE-MIT.txt`) | `6ef927ca252b40bc9b5d8843d84137334362f8aa1acbb1bf8c15a02affd25b2b` |
| EffectCraft | `src/assets/app-icons/effectcraft.png` (128x128) | [storytold/effectcraft@d60cb71e297a](https://github.com/storytold/effectcraft/blob/d60cb71e297a4bb76851b27b48c0014b56ccab69/assets/app-icon/hicolor/128x128/apps/ai.storyteller.effectcraft.png) | MIT OR Apache-2.0 (`effectcraft.LICENSE.txt`, `effectcraft.LICENSE-MIT.txt`) | `4ad7caa4f43b4cbf3bcddda0ed23e471d392efbe8d0b4ce18107b5849f20dd2d` |
| DesignCraft | `src/assets/app-icons/designcraft.png` (128x128) | [storytold/designcraft@14e677b24216](https://github.com/storytold/designcraft/blob/14e677b24216396e398823ff034d9882322362d1/assets/app-icon/hicolor/128x128/apps/ai.storyteller.designcraft.png) | MIT OR Apache-2.0 (`designcraft.LICENSE.txt`, `designcraft.LICENSE-MIT.txt`) | `0895763da92947cb6b1d38a018a6c7ffcda1cbb714a58f2e9bc08e0e2a2b94b4` |
| GridCraft | `src/assets/app-icons/gridcraft.png` (128x128) | [storytold/gridcraft@fb823899c57b](https://github.com/storytold/gridcraft/blob/fb823899c57b41703edcad2b6476cf4b8a01dfc4/assets/app-icon/hicolor/128x128/apps/ai.storyteller.gridcraft.png) | MIT OR Apache-2.0 (`gridcraft.LICENSE.txt`, `gridcraft.LICENSE-MIT.txt`) | `01ef35442b8d466b52068fda67ab740309321c8bc813b62b9207dfc4116208e7` |

Not bundled (generic Lucide icons are shown instead):

| App | Reason |
|---|---|
| SoundCraft, WordCraft, DeckCraft, CADCraft | No `assets/app-icon/LICENSE.txt` in the upstream repository on 2026-10-08, so no explicit icon licence was found. Not assumed. |
| ArtCraft | The ArtCraft name, logo and mark are trademarks of the ArtCraft Team and are **not** open source (`docs/brand/LICENSE-brand.txt` in the Craft repos; ArtCraft's own `LICENSE.md` forbids using its logo without permission). |

The ArtCraft wordmark/mark files in upstream `docs/brand/` are never copied into CraftHub.

Names such as PhotoCraft or ArtCraft are used only to identify the upstream applications CraftHub
manages. CraftHub is an independent, unofficial project and is not affiliated with or endorsed by
Storytold or ArtCraft; the icons do not imply otherwise.

To update an icon: re-run the licence check, copy the new file unmodified, and update the commit and
SHA-256 above.

## Other bundled third-party material

| Component | Use | License |
|---|---|---|
| [Lucide](https://lucide.dev) icons (`lucide-react`) | UI icons, generic app icons | ISC |
| npm and Cargo dependencies | Application code | See each package; listed in `package-lock.json` / `Cargo.lock` |
