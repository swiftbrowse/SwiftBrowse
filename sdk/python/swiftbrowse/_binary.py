"""Finds the `swiftbrowse` binary: an explicit path, $SWIFTBROWSE_BIN, the one
shipped in this package's platform wheel, then PATH."""

import os
import shutil
import sys
from typing import Optional

_EXE = "swiftbrowse.exe" if sys.platform == "win32" else "swiftbrowse"


def known_binary(explicit: Optional[str] = None) -> Optional[str]:
    """The binary from an explicit path, $SWIFTBROWSE_BIN or the platform wheel, if any."""
    if explicit:
        return explicit
    if os.environ.get("SWIFTBROWSE_BIN"):
        return os.environ["SWIFTBROWSE_BIN"]
    bundled = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bin", _EXE)
    return bundled if os.path.isfile(bundled) else None


def find_binary(explicit: Optional[str] = None) -> str:
    return known_binary(explicit) or shutil.which("swiftbrowse") or "swiftbrowse"
