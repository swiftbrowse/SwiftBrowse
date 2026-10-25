"""`swiftbrowse …` (and `python -m swiftbrowse …`): runs the native binary with
the same arguments, stdio and exit code."""

import os
import subprocess
import sys

from ._binary import find_binary, known_binary

# Set when this script fell back to `swiftbrowse` on PATH. Seeing it here means PATH
# led back to a launcher script (often this one), which would start itself forever.
_LOOP = "SWIFTBROWSE_LAUNCHER_FALLBACK"
_MISSING = (
    "swiftbrowse: no binary for this platform: this install has no platform wheel. "
    "Install swiftbrowse on a supported platform (Linux x86_64/aarch64, macOS, Windows x64) "
    "or set SWIFTBROWSE_BIN to a swiftbrowse binary."
)


def main() -> None:
    if os.environ.get(_LOOP):
        sys.exit(_MISSING)
    binary = find_binary()
    if known_binary() is None:
        os.environ[_LOOP] = "1"
    args = [binary, *sys.argv[1:]]
    try:
        if sys.platform == "win32":
            sys.exit(subprocess.call(args))
        os.execv(binary, args) if os.path.isabs(binary) else os.execvp(binary, args)
    except FileNotFoundError:
        sys.exit(_MISSING)
    except KeyboardInterrupt:
        sys.exit(130)


if __name__ == "__main__":
    main()
