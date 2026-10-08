# End-to-end smoke checklist (Windows x64)

Run on a real machine with network access. Record date, CraftHub version and results.

| # | Step | Expected |
|---|------|----------|
| 1 | Launch CraftHub (`target\release\crafthub.exe` or installed copy) | Window opens; Home shows hero + disclaimer |
| 2 | All Apps | 12 Craft cards + ArtCraft under "Separate AI studio" |
| 3 | Check for updates | Connectivity shows Online; latest versions appear; SoundCraft and ArtCraft show Unavailable with a reason and a Releases button |
| 4 | Install PhotoCraft (or "Show all versions" → install an older version) | Real byte progress, Verifying, Unpacking, Activating; success toast |
| 5 | Open PhotoCraft | PhotoCraft window appears; card shows Running |
| 6 | Click Update while it runs | Update disabled with "Close PhotoCraft to update it" |
| 7 | Close PhotoCraft, Update | Updates; drawer shows previous version kept for rollback |
| 8 | Roll back, then roll forward | Version switches without downloading |
| 9 | Disconnect network, restart CraftHub, Open PhotoCraft | Opens; connectivity shows cached/offline honestly |
| 10 | Uninstall (confirm) | Files removed; `%APPDATA%\Photocraft` untouched; app folder gone or kept files reported |
| 11 | Settings → History | Each action listed with outcome |

| 12 | Close the window during an install | Window hides to the tray; the install finishes |
| 13 | Tray → Quit during an install | "Quit while installing?" dialog; *Cancel and quit* rolls back and exits |
| 14 | Settings → Notify mode, then a new upstream release | One Windows notification; clicking it opens the app details |
| 15 | Settings → Install location → Change… (empty folder) | New installs go there; existing apps stay and still update |
| 16 | Run `crafthub-cli install <app>` while the app is installing the same app | The CLI reports *busy* |
| 17 | Install an app and choose a different empty local folder | Dialog shows parent and final app folder; install succeeds there; unrelated files are not touched |
| 18 | Install with “Create desktop shortcut” enabled | A `.lnk` appears on the actual Windows Desktop and opens the verified executable |
| 19 | Update an app with a managed shortcut | No location prompt; the shortcut remains and targets the new version |
| 20 | Create a user shortcut with the same visible name, then uninstall CraftHub's app | The user shortcut remains; only the CraftHub-owned shortcut is removed |
| 21 | Run an automatic update for an app without a shortcut | No dialog and no new shortcut |

## CLI equivalent

```powershell
cargo build -p crafthub-core --bin crafthub-cli
$cli = ".\target\debug\crafthub-cli.exe"
& $cli check
& $cli install gridcraft 0.1.0
& $cli launch gridcraft       # close it before updating
& $cli update gridcraft
& $cli rollback gridcraft
& $cli history
& $cli uninstall gridcraft
```

Set `CRAFTHUB_ROOT=<folder>` to run against an isolated apps/data folder.

## Manual checks: tray menu and notification clicks

These two areas can't be driven reliably by UI Automation (the tray menu is a native popup;
toast activation comes from Windows), so check them by hand before each release.

**Setup without touching your real apps:** quit any running CraftHub (tray → Quit). In a
PowerShell window run `$env:CRAFTHUB_ROOT = "$env:TEMP\crafthub-manual"`, install an *old* version
with the CLI (`crafthub-cli install gridcraft 0.1.0`), then start CraftHub **from that same window**
so it uses the isolated folder. Uninstalled builds show notifications as coming from
"Windows PowerShell"; the installed app shows "CraftHub".

| # | Action | Expected |
|---|---|---|
| T1 | Find the CraftHub icon (it may be under ^ "Show hidden icons") and right-click it | Menu: *Open CraftHub*, *Check for updates*, separator, *Quit CraftHub* |
| T2 | Close the window, then menu → *Open CraftHub* | Window reappears and gets focus |
| T3 | Left-click the icon while the window is hidden | Window reappears |
| T4 | Menu → *Check for updates* with an update not yet announced | One toast "GridCraft 0.3.0 is available" |
| T5 | Menu → *Check for updates* again | Toast "1 update available." (no repeat of T4's per-version toast) |
| T6 | Disconnect the network, menu → *Check for updates* | Toast "CraftHub could not check for updates" with a reason |
| T7 | No installs running, menu → *Quit CraftHub* | CraftHub exits within ~1 s; the icon disappears |
| T8 | Start installing DeckCraft, menu → *Quit CraftHub* | Window opens with "Quit while installing?" naming DeckCraft |
| T9 | In that dialog choose *Keep working* | Dialog closes; the install continues and completes |
| T10 | Start another install, menu → *Quit* → *Cancel and quit* | CraftHub exits; after restarting, the app is not installed, `Staging`/`Downloads` are empty, history shows *cancelled* |
| N1 | Click a single-app toast (T4) | CraftHub comes to the front on Updates with that app's details open, even if the window was hidden |
| N2 | Click a multi-update toast (install two old versions first) | CraftHub opens on the Updates page |
| N3 | In Automatic mode, click the "CraftHub installed updates" toast | CraftHub opens on the Installed page |
| N4 | Settings → turn notifications off, wait for a background check (or restart with *check on start-up*) | No toast; the Updates badge still updates |
| N5 | Quit CraftHub, then click an old toast in the Windows notification centre | Nothing breaks. Known limitation: clicks only navigate while CraftHub is running |

Afterwards: quit CraftHub, `Remove-Item Env:CRAFTHUB_ROOT`, and delete `%TEMP%\crafthub-manual`.
