"""SwiftBrowse SDK for Python: drives the `swiftbrowse` binary over its SDK
protocol (JSON lines on stdin/stdout). Standard library only.

    from swiftbrowse import launch

    with launch() as browser:
        page = browser.new_session()
        page.navigate("https://example.com")
        page.click("More information", expect={"type": "url_contains", "value": "iana"})
        page.expect({"type": "title_contains", "value": "IANA"})
"""

import json
import os
import subprocess
import threading
from typing import List, Optional, Sequence, Tuple, Union

from ._generated import PROTOCOL, Operations

__all__ = ["launch", "Browser", "Session", "SwiftBrowseError"]


class SwiftBrowseError(Exception):
    """An operation failed (``result["ok"]`` is false). ``result`` holds the full result."""

    def __init__(self, result: dict):
        error = result.get("error") or {}
        notes = result.get("notes") or []
        extra = f" [{'; '.join(notes)}]" if notes else ""
        super().__init__(f"{result.get('op')} failed ({error.get('kind', 'error')}): {error.get('message', 'unknown error')}{extra}")
        self.kind = error.get("kind")
        self.result = result


def launch(
    executable: Optional[str] = None,
    *,
    test_defaults: bool = True,
    headed: bool = False,
    no_daemon: bool = False,
    chromium: Optional[str] = None,
    evidence_dir: Optional[str] = None,
    timeout_ms: Optional[int] = None,
    allow_hosts: Sequence[str] = (),
    block_hosts: Sequence[str] = (),
    upload_dirs: Sequence[str] = (),
    allow_file_urls: bool = False,
    throw_on_error: bool = True,
) -> "Browser":
    """Starts a ``swiftbrowse sdk`` process; sessions attach to the warm browser daemon.

    ``test_defaults`` (on by default): no approval prompts for "Delete"/"Pay"
    clicks, ``evaluate`` allowed, uploads from the working directory.
    """
    binary = executable or os.environ.get("SWIFTBROWSE_BIN", "swiftbrowse")
    args = [binary, "sdk"]
    if test_defaults:
        args.append("--test-defaults")
    if headed:
        args.append("--headed")
    if no_daemon:
        args.append("--no-daemon")
    if chromium:
        args += ["--chromium", chromium]
    if evidence_dir:
        args += ["--evidence-dir", evidence_dir]
    if timeout_ms:
        args += ["--timeout-ms", str(timeout_ms)]
    for host in allow_hosts:
        args += ["--allow-host", host]
    for host in block_hosts:
        args += ["--block-host", host]
    for d in upload_dirs:
        args += ["--allow-upload-dir", d]
    if allow_file_urls:
        args.append("--allow-file-urls")
    browser = Browser(subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1), throw_on_error)
    hello = browser.request("hello")
    if hello.get("protocol") != PROTOCOL:
        browser.close()
        raise RuntimeError(f"swiftbrowse speaks SDK protocol {hello.get('protocol')}, this SDK expects {PROTOCOL}; update one of them")
    browser.version = hello.get("version")
    return browser


class Browser:
    def __init__(self, proc: subprocess.Popen, throw_on_error: bool):
        self._proc = proc
        self._lock = threading.Lock()
        self._next_id = 0
        self.throw_on_error = throw_on_error
        self.version: Optional[str] = None

    def request(self, method: str, params: Optional[dict] = None):
        """Sends one protocol request and waits for its reply."""
        with self._lock:
            self._next_id += 1
            request_id = self._next_id
            assert self._proc.stdin and self._proc.stdout
            self._proc.stdin.write(json.dumps({"id": request_id, "method": method, "params": params or {}}) + "\n")
            self._proc.stdin.flush()
            while True:
                line = self._proc.stdout.readline()
                if not line:
                    raise RuntimeError(f"swiftbrowse exited (code {self._proc.poll()})")
                msg = json.loads(line)
                if msg.get("id") == request_id:
                    break
        if "error" in msg:
            raise RuntimeError(msg["error"]["message"])
        return msg["result"]

    def new_session(self, *, headless: Optional[bool] = None, viewport: Optional[Tuple[int, int]] = None, throw_on_error: Optional[bool] = None) -> "Session":
        """An isolated session: its own cookies, storage and tabs."""
        params = {}
        if headless is not None:
            params["headless"] = headless
        if viewport:
            params["viewport"] = list(viewport)
        info = self.request("create_session", params)
        return Session(self, info, self.throw_on_error if throw_on_error is None else throw_on_error)

    def close(self) -> None:
        """Closes every session and stops the process."""
        if self._proc.poll() is None:
            assert self._proc.stdin
            self._proc.stdin.close()
            try:
                self._proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._proc.kill()

    def __enter__(self) -> "Browser":
        return self

    def __exit__(self, *exc) -> None:
        self.close()


class Session(Operations):
    def __init__(self, browser: Browser, info: dict, throw_on_error: bool):
        self.browser = browser
        self.id = info["session_id"]
        self.info = info
        self.throw_on_error = throw_on_error

    def run(self, op: str, args: Optional[dict] = None) -> dict:
        """Runs any operation by name with its arguments."""
        result = self.browser.request("execute", {"session": self.id, "op": op, "args": args or {}})
        if not result["ok"] and self.throw_on_error:
            raise SwiftBrowseError(result)
        return result

    def expect(self, condition: Union[dict, List[dict]], *, timeout_ms: int = 5000, name: Optional[str] = None) -> dict:
        """Auto-waiting assertion: passes as soon as every condition holds, fails after ``timeout_ms``."""
        conditions = condition if isinstance(condition, list) else [condition]
        args = {"conditions": conditions, "timeout_ms": timeout_ms}
        if name:
            args["name"] = name
        return self.run("assert_page_state", args)

    def text(self) -> str:
        """The page as compact text: URL, title, headings, interactive elements with refs."""
        return self.run("get_page_snapshot", {"format": "text"})["data"]["text"]

    def close(self) -> None:
        self.browser.request("close_session", {"session": self.id})

    def __enter__(self) -> "Session":
        return self

    def __exit__(self, *exc) -> None:
        self.close()
