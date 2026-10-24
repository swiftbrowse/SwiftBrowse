"""The Python SDK end to end against the example app. Runs with
`python -m unittest` or pytest. Needs a built `swiftbrowse`
(SWIFTBROWSE_BIN, default ../../target/debug/swiftbrowse)."""

import functools
import http.server
import os
import threading
import unittest

from swiftbrowse import SwiftBrowseError, launch

HERE = os.path.dirname(os.path.abspath(__file__))
SITE = os.path.join(HERE, "..", "..", "..", "examples", "e2e", "site")
os.environ.setdefault("SWIFTBROWSE_BIN", os.path.join(HERE, "..", "..", "..", "target", "debug", "swiftbrowse"))


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


class SdkTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        handler = functools.partial(Quiet, directory=SITE)
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.base = f"http://127.0.0.1:{cls.server.server_address[1]}/"
        cls.browser = launch()

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.server.shutdown()

    def test_adds_and_completes_a_todo(self):
        with self.browser.new_session() as page:
            page.navigate(self.base + "index.html")
            page.fill("New todo", "Write the SDK")
            page.press_key("Enter", expect={"type": "visible", "target": {"by": "role", "role": "checkbox", "name": "Write the SDK"}})
            page.check({"by": "role", "role": "checkbox", "name": "Write the SDK"})
            page.expect({"type": "text_equals", "target": {"by": "test_id", "id": "count"}, "text": "0 items left"})
            self.assertRegex(page.text(), r'checkbox "Write the SDK" \[e\d+\] checked')

    def test_mocks_the_network(self):
        with self.browser.new_session() as page:
            page.route(url="**/api/todos", json=[{"title": "From the mock"}])
            page.navigate(self.base + "api.html")
            page.expect([
                {"type": "text_visible", "text": "From the mock"},
                {"type": "request_seen", "url_contains": "/api/todos", "method": "GET"},
            ])
            self.assertEqual(page.evaluate('document.querySelectorAll("#list li").length')["data"]["value"], 1)

    def test_failures_raise_with_the_result(self):
        with self.browser.new_session() as page:
            page.navigate(self.base + "index.html")
            with self.assertRaises(SwiftBrowseError) as caught:
                page.click("No such button", timeout_ms=300)
            self.assertEqual(caught.exception.kind, "timeout")
            self.assertEqual(caught.exception.result["op"], "click")

    def test_sessions_are_isolated(self):
        a, b = self.browser.new_session(), self.browser.new_session()
        a.navigate(self.base + "index.html")
        a.fill("New todo", "Only in A")
        a.press_key("Enter", expect={"type": "text_visible", "text": "Only in A"})
        b.navigate(self.base + "index.html")
        b.expect({"type": "text_absent", "text": "Only in A"})
        a.close()
        b.close()


if __name__ == "__main__":
    unittest.main()
