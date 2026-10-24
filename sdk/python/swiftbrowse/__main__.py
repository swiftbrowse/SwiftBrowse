"""`swiftbrowse …` (and `python -m swiftbrowse …`): runs the native binary with
the same arguments, stdio and exit code."""

import os
import subprocess
import sys

from ._binary import find_binary


def main() -> None:
    binary = find_binary()
    args = [binary, *sys.argv[1:]]
    try:
        if sys.platform == "win32":
            sys.exit(subprocess.call(args))
        os.execv(binary, args) if os.path.isabs(binary) else os.execvp(binary, args)
    except FileNotFoundError:
        sys.exit(f"swiftbrowse: no binary for this platform; install a platform wheel or set SWIFTBROWSE_BIN")
    except KeyboardInterrupt:
        sys.exit(130)


if __name__ == "__main__":
    main()
