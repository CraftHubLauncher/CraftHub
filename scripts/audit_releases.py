"""Usage: python -I scripts/audit_releases.py [app ...]

Re-audit Windows x64 portable ZIPs by reading only their central directory via HTTP
range requests (no full download). Prints asset, digest presence, entries, root folder,
exe presence and any portable-mode marker files."""
import io
import json
import sys
from pathlib import Path
import urllib.request
import zipfile

UA = {"User-Agent": "CraftHub-audit", "Accept": "application/vnd.github+json"}
CATALOG = Path(__file__).resolve().parent.parent / "catalog" / "apps.json"


def catalog_variants():
    """(app id, [(asset stem, executable), ...]) for every app with a Windows adapter."""
    data = json.loads(CATALOG.read_text(encoding="utf-8"))
    for app in data["applications"]:
        w = app.get("windowsX64")
        if w:
            yield app["id"], [(v["assetStem"], v["executable"]) for v in w["variants"]]


class RangeFile(io.RawIOBase):
    def __init__(self, url, size):
        self.url, self.size, self.pos, self.requests = url, size, 0, 0

    def seekable(self):
        return True

    def readable(self):
        return True

    def tell(self):
        return self.pos

    def seek(self, off, whence=0):
        self.pos = {0: off, 1: self.pos + off, 2: self.size + off}[whence]
        return self.pos

    def readinto(self, b):
        if self.pos >= self.size:
            return 0
        end = min(self.size, self.pos + len(b)) - 1
        req = urllib.request.Request(self.url, headers={"User-Agent": "CraftHub-audit", "Range": f"bytes={self.pos}-{end}"})
        with urllib.request.urlopen(req, timeout=60) as r:
            if r.status != 206:
                raise RuntimeError(f"range not honoured: {r.status}")
            data = r.read()
        self.requests += 1
        n = len(data)
        b[:n] = data
        self.pos += n
        return n


def main():
    out = {}
    wanted = set(sys.argv[1:])
    for app, variants in catalog_variants():
        if wanted and app not in wanted:
            continue
        req = urllib.request.Request(f"https://api.github.com/repos/storytold/{app}/releases?per_page=5", headers=UA)
        rels = json.load(urllib.request.urlopen(req, timeout=60))
        stable = [r for r in rels if not r["draft"] and not r["prerelease"] and "-" not in r["tag_name"]]
        if not stable:
            out[app] = {"status": "no stable release"}
            continue
        rel = stable[0]
        ver = rel["tag_name"][1:]
        assets = {a["name"]: a for a in rel["assets"]}
        found = [(stem, exe) for stem, exe in variants if f"{stem}-{ver}-windows-x64-portable.zip" in assets]
        if len(found) != 1:
            out[app] = {"tag": rel["tag_name"], "status": f"{len(found)} audited variants match; expected exactly 1"}
            continue
        stem, exe = found[0]
        name = f"{stem}-{ver}-windows-x64-portable.zip"
        a = assets[name]
        f = RangeFile(a["browser_download_url"], a["size"])
        z = zipfile.ZipFile(io.BufferedReader(f, buffer_size=256 * 1024))
        names = z.namelist()
        roots = sorted({n.split("/")[0] for n in names})
        root = f"{stem}-{ver}-windows-x64-portable"
        markers = [n for n in names if n.lower().endswith(("portable.txt", "portable", ".portable", "portable.ini"))]
        symlinks = [i.filename for i in z.infolist() if (i.external_attr >> 16) & 0o170000 == 0o120000]
        out[app] = {
            "tag": rel["tag_name"], "published": rel["published_at"], "asset": name, "size": a["size"],
            "digest": a.get("digest"), "sha256sums": "SHA256SUMS.txt" in assets,
            "entries": len(names), "uncompressed": sum(i.file_size for i in z.infolist()),
            "roots": roots, "root_ok": roots == [root],
            "exe_ok": f"{root}/{exe}" in names, "markers": markers, "symlinks": symlinks,
            "nested_dirs": sorted({n for n in names if n.count("/") > 1})[:5],
            "range_requests": f.requests,
        }
    json.dump(out, sys.stdout, indent=1)


main()
