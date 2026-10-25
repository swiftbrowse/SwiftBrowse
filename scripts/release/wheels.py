"""Builds the PyPI wheels for a release: one per built binary (the SDK plus the
binary in swiftbrowse/bin/), and a pure-Python wheel that uses a `swiftbrowse`
on PATH. Standard library only.

Usage: python scripts/release/wheels.py <version> <binaries-dir> <out-dir>
"""

import base64
import hashlib
import json
import os
import shutil
import sys
import zipfile

version, binaries, out = sys.argv[1:4]
root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sdk = os.path.join(root, "sdk", "python")
platforms = json.load(open(os.path.join(root, "scripts", "release", "platforms.json")))
shutil.rmtree(out, ignore_errors=True)
os.makedirs(out)

readme = open(os.path.join(sdk, "README.md"), encoding="utf-8").read()
metadata = f"""Metadata-Version: 2.1
Name: swiftbrowse
Version: {version}
Summary: The AI browser for browser automation and end-to-end testing: Python SDK
Home-page: https://github.com/swiftbrowse/swiftbrowse
Project-URL: Documentation, https://steady-ember-82q7.here.now/
Project-URL: Source, https://github.com/swiftbrowse/swiftbrowse
Project-URL: Issues, https://github.com/swiftbrowse/swiftbrowse/issues
License: Apache-2.0
Keywords: browser,automation,testing,e2e,playwright,mcp,ai-agents,chromium
Requires-Python: >=3.8
Description-Content-Type: text/markdown

{readme}"""
entry_points = "[console_scripts]\nswiftbrowse = swiftbrowse.__main__:main\n"
sources = sorted(f for f in os.listdir(os.path.join(sdk, "swiftbrowse")) if f.endswith(".py"))


def digest(data: bytes) -> str:
    return "sha256=" + base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()


def build(tag: str, binary=None, exe=None) -> str:
    name = f"swiftbrowse-{version}-py3-none-{tag}.whl"
    dist = f"swiftbrowse-{version}.dist-info"
    files = [(f"swiftbrowse/{f}", open(os.path.join(sdk, "swiftbrowse", f), "rb").read(), 0o644) for f in sources]
    if binary:
        files.append((f"swiftbrowse/bin/{exe}", open(binary, "rb").read(), 0o755))
    wheel = f"Wheel-Version: 1.0\nGenerator: swiftbrowse-release\nRoot-Is-Purelib: {'false' if binary else 'true'}\nTag: py3-none-{tag}\n"
    files += [
        (f"{dist}/METADATA", metadata.encode(), 0o644),
        (f"{dist}/WHEEL", wheel.encode(), 0o644),
        (f"{dist}/entry_points.txt", entry_points.encode(), 0o644),
        (f"{dist}/licenses/LICENSE", open(os.path.join(root, "LICENSE"), "rb").read(), 0o644),
    ]
    record = "".join(f"{n},{digest(d)},{len(d)}\n" for n, d, _ in files) + f"{dist}/RECORD,,\n"
    files.append((f"{dist}/RECORD", record.encode(), 0o644))
    with zipfile.ZipFile(os.path.join(out, name), "w", zipfile.ZIP_DEFLATED) as z:
        for n, d, mode in files:
            info = zipfile.ZipInfo(n, date_time=(2026, 1, 1, 0, 0, 0))
            info.external_attr = (0o100000 | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, d)
    return name


built = [build("any")]
for p in platforms:
    binary = os.path.join(binaries, p["id"], p["exe"])
    if os.path.exists(binary):
        built.append(build(p["wheel"], binary, p["exe"]))
    else:
        print(f"no binary for {p['id']}; skipping its wheel")
print("\n".join(built))
