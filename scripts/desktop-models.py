#!/usr/bin/env python3
"""Firefox Translations models (Mozilla, MPL-2.0) for the desktop app on Linux
and Windows: English to each language, one consistent set per language (the
model and its vocabularies from the same version and variant; mixing the
desktop and Android Korean sets gives nonsense). Writes en-<lang>.model.bin,
en-<lang>.src.spm, en-<lang>.trg.spm and SHA256SUMS into the folder given,
to be put on a release of this repo (Mozilla's servers are for Firefox).
    scripts/desktop-models.py <out-dir> [lang ...]
"""
import hashlib, json, os, subprocess, sys, urllib.request

INDEX = "https://firefox.settings.services.mozilla.com/v1/buckets/main/collections/translations-models-v2/records"
CDN = "https://firefox-settings-attachments.cdn.mozilla.net/"
LANGS = ["fr", "de", "es", "zh-Hans", "ja", "ko", "ar", "vi"]
# Smaller, scored better where both exist (Korean): the memory-light variant first.
PREFER = ["base-memory", "base", "tiny"]

def ver(v):
    return [int(p) if p.isdigit() else 0 for p in (v or "0").replace("a", ".").split(".")]

def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "ccez-keys model fetch"})
    return urllib.request.urlopen(req, timeout=300).read()

def main():
    out = sys.argv[1]
    langs = sys.argv[2:] or LANGS
    os.makedirs(out, exist_ok=True)
    recs = json.loads(get(INDEX))["data"]
    for lang in langs:
        mine = [r for r in recs if r.get("sourceLanguage") == "en" and r.get("targetLanguage") == lang and ver(r.get("version"))[0] == 3]
        if not mine:
            print(f"{lang}: no model"); continue
        # One variant and one version for every file of the set.
        sets = {}
        for r in mine:
            # Android's and the desktop's sets are told apart only by this.
            sets.setdefault((r.get("architecture") or "base", r["version"], r.get("filter_expression") or ""), {})[r["fileType"]] = r
        usable = [(k, s) for k, s in sets.items() if "model" in s and ("vocab" in s or ("srcvocab" in s and "trgvocab" in s))]
        usable.sort(key=lambda ks: (PREFER.index(ks[0][0]) if ks[0][0] in PREFER else 9, [-x for x in ver(ks[0][1])]))
        if not usable:
            print(f"{lang}: no whole set"); continue
        (arch, version, where), s = usable[0]
        files = {"model.bin": s["model"], "src.spm": s.get("srcvocab", s.get("vocab")), "trg.spm": s.get("trgvocab", s.get("vocab"))}
        for suffix, r in files.items():
            dest = os.path.join(out, f"en-{lang}.{suffix}")
            raw = get(CDN + r["attachment"]["location"])
            if raw[:4] == b"\x28\xb5\x2f\xfd":
                raw = subprocess.run(["zstd", "-d", "-c"], input=raw, capture_output=True, check=True).stdout
            if r.get("decompressedHash") and hashlib.sha256(raw).hexdigest() != r["decompressedHash"]:
                sys.exit(f"{lang} {suffix}: hash mismatch")
            open(dest, "wb").write(raw)
        print(f"{lang}: {arch} {version} {where or 'desktop'}")
    with open(os.path.join(out, "SHA256SUMS"), "w") as f:
        for name in sorted(os.listdir(out)):
            if name.startswith("en-"):
                f.write(f"{hashlib.sha256(open(os.path.join(out, name), 'rb').read()).hexdigest()}  {name}\n")

main()
