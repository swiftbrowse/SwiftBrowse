"""Finds the `swiftbrowse` binary: an explicit path, $SWIFTBROWSE_BIN, the one
shipped in this package's platform wheel, then PATH."""

import os
import shutil
import sys
from typing import Optional

_EXE = "swiftbrowse.exe" if sys.platform == "win32" else "swiftbrowse"


def find_binary(explicit: Optional[str] = None) -> str:
    if explicit:
        return explicit
    if os.environ.get("SWIFTBROWSE_BIN"):
        return os.environ["SWIFTBROWSE_BIN"]
    bundled = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bin", _EXE)
    if os.path.isfile(bundled):
        return bundled
    return shutil.which("swiftbrowse") or "swiftbrowse"
