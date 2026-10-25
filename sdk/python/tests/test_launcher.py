"""The `swiftbrowse` command without a platform wheel: the `swiftbrowse` on PATH
can be this same launcher script, which must fail with a clear message instead
of starting itself forever."""

import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
PACKAGE_ROOT = os.path.join(HERE, "..")


@unittest.skipIf(sys.platform == "win32", "uses a shell script on PATH")
class LauncherTest(unittest.TestCase):
    def test_launcher_that_finds_itself_on_path_stops(self):
        with tempfile.TemporaryDirectory() as d:
            script = os.path.join(d, "swiftbrowse")
            with open(script, "w") as f:
                f.write(f'#!/bin/sh\nexec "{sys.executable}" -m swiftbrowse "$@"\n')
            os.chmod(script, 0o755)
            env = {k: v for k, v in os.environ.items() if k != "SWIFTBROWSE_BIN"}
            env["PATH"] = d + os.pathsep + env.get("PATH", "")
            env["PYTHONPATH"] = PACKAGE_ROOT
            r = subprocess.run([script, "--version"], env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(r.returncode, 1)
        self.assertIn("no binary for this platform", r.stderr)


if __name__ == "__main__":
    unittest.main()
