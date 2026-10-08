"""Fetch each Craft app's official app icon (pinned to a commit) only if its icon LICENSE
says MIT/Apache-2.0. Writes PNGs to argv[1] and a JSON provenance record to argv[2]."""
import hashlib
import json
import sys
import urllib.request

APPS = ["photocraft", "vectorcraft", "filmcraft", "lightcraft", "pdfcraft", "effectcraft",
        "designcraft", "soundcraft", "wordcraft", "gridcraft", "deckcraft", "cadcraft"]
H = {"User-Agent": "CraftHub-audit", "Accept": "application/vnd.github+json"}


def get(url, raw=False):
    req = urllib.request.Request(url, headers=H if not raw else {"User-Agent": "CraftHub-audit"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


out_dir, record = sys.argv[1], {}
for app in APPS:
    entry = {"app": app}
    try:
        sha = json.loads(get(f"https://api.github.com/repos/storytold/{app}/commits/main"))["sha"]
        lic = get(f"https://raw.githubusercontent.com/storytold/{app}/{sha}/assets/app-icon/LICENSE.txt", raw=True).decode("utf-8", "replace")
        ok = ("MIT" in lic and "Apache" in lic and "trademark" not in lic.lower() and "not open source" not in lic.lower())
        entry.update(commit=sha, license_ok=ok, license_first_line=lic.strip().splitlines()[0])
        if not ok:
            entry["skipped"] = "icon license is not plainly MIT/Apache-2.0"
            record[app] = entry
            continue
        for size in ("128x128", "64x64"):
            path = f"assets/app-icon/hicolor/{size}/apps/ai.storyteller.{app}.png"
            try:
                data = get(f"https://raw.githubusercontent.com/storytold/{app}/{sha}/{path}", raw=True)
            except Exception:
                continue
            if data[:8] != b"\x89PNG\r\n\x1a\n":
                continue
            open(f"{out_dir}/{app}.png", "wb").write(data)
            open(f"{out_dir}/{app}.LICENSE.txt", "w", encoding="utf-8", newline="\n").write(lic)
            entry.update(source=f"https://github.com/storytold/{app}/blob/{sha}/{path}", size=size,
                         bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
            break
        else:
            entry["skipped"] = "no PNG icon found"
    except Exception as e:  # noqa: BLE001
        entry["skipped"] = f"error: {e}"
    record[app] = entry
json.dump(record, open(sys.argv[2], "w", encoding="utf-8"), indent=1)
for a, e in record.items():
    print(a, e.get("size"), e.get("license_ok"), e.get("skipped", ""), e.get("license_first_line", "")[:60])
