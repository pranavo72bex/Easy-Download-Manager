#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Tests of scripts/update-solver.py against a stand-in releases server (no network, no YouTube).

    python3 scripts/test-update-solver.py
"""
import hashlib, http.server, importlib.util, json, pathlib, tempfile, threading, unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("update_solver", ROOT / "scripts/update-solver.py")
update = importlib.util.module_from_spec(spec)
spec.loader.exec_module(update)

OLD_LIB = "a" * 64
OLD_CORE = "b" * 64
SOURCE = f'''// before
/// The solver's pinned release.
const SOLVER_VERSION: &str = "0.8.0";
const SOLVER_LIB: (&str, &str) = (
    "yt.solver.lib.min.js",
    "{OLD_LIB}",
);
const SOLVER_CORE: (&str, &str) = (
    "yt.solver.core.min.js",
    "{OLD_CORE}",
);
fn base() -> String {{
    format!("https://github.com/someone/scripts/releases/download/{{SOLVER_VERSION}}")
}}
// after
'''
LIB = b"var lib=1;/* new lib */"
CORE = b"var core=2;/* new core */"


class Releases(http.server.BaseHTTPRequestHandler):
    tag = "0.9.0"
    assets = {"yt.solver.lib.min.js": LIB, "yt.solver.core.min.js": CORE}

    def log_message(self, *args):
        pass

    def reply(self, status, body, kind="application/octet-stream"):
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/latest":
            return self.reply(200, json.dumps({"tag_name": self.tag}).encode(), "application/json")
        prefix = f"/download/{self.tag}/"
        if self.path.startswith(prefix) and self.path[len(prefix):] in self.assets:
            return self.reply(200, self.assets[self.path[len(prefix):]])
        self.reply(404, b"not found")


class UpdateSolver(unittest.TestCase):
    def setUp(self):
        Releases.tag = "0.9.0"
        Releases.assets = {"yt.solver.lib.min.js": LIB, "yt.solver.core.min.js": CORE}
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Releases)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        self.dir = tempfile.TemporaryDirectory()
        self.source = pathlib.Path(self.dir.name) / "youtube.rs"
        self.source.write_text(SOURCE, encoding="utf-8")

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.dir.cleanup()

    def run_update(self, *flags):
        return update.main(["--source", str(self.source), "--api", f"{self.url}/latest",
                            "--assets", f"{self.url}/download", *flags])

    def test_a_newer_release_rewrites_the_pin_and_only_the_pin(self):
        summary = pathlib.Path(self.dir.name) / "summary.md"
        self.assertEqual(self.run_update("--apply", "--summary-file", str(summary)), 0)
        text = self.source.read_text(encoding="utf-8")
        self.assertEqual(text, SOURCE.replace('"0.8.0"', '"0.9.0"')
                         .replace(OLD_LIB, hashlib.sha256(LIB).hexdigest())
                         .replace(OLD_CORE, hashlib.sha256(CORE).hexdigest()))
        body = summary.read_text(encoding="utf-8")
        self.assertIn("0.8.0", body)
        self.assertIn("0.9.0", body)
        self.assertIn("compare/0.8.0...0.9.0", body)
        self.assertIn(hashlib.sha256(LIB).hexdigest(), body)

    def test_the_same_or_an_older_release_changes_nothing(self):
        for tag in ("0.8.0", "0.7.9", "v0.8.0"):
            Releases.tag = tag
            self.assertEqual(self.run_update("--apply"), 0)
            self.assertEqual(self.source.read_text(encoding="utf-8"), SOURCE)

    def test_check_says_a_newer_release_exists_and_changes_nothing(self):
        self.assertEqual(self.run_update("--check"), 10)
        self.assertEqual(self.source.read_text(encoding="utf-8"), SOURCE)
        Releases.tag = "0.8.0"
        self.assertEqual(self.run_update("--check"), 0)

    def test_a_tag_with_a_v_and_four_parts_is_understood_and_others_refused(self):
        self.assertEqual(update.numeric("v1.2.3.4"), ("1.2.3.4", (1, 2, 3, 4)))
        for bad in ("latest", "1.2.x", "1.2; rm -rf", "", "../1.0"):
            with self.assertRaises(update.Fail):
                update.numeric(bad)

    def test_a_missing_script_fails_and_changes_nothing(self):
        del Releases.assets["yt.solver.core.min.js"]
        self.assertEqual(self.run_update("--apply"), 2)
        self.assertEqual(self.source.read_text(encoding="utf-8"), SOURCE)

    def test_an_error_page_in_place_of_a_script_is_refused(self):
        Releases.assets["yt.solver.lib.min.js"] = b"<!DOCTYPE html><html>Sign in</html>"
        self.assertEqual(self.run_update("--apply"), 2)
        Releases.assets["yt.solver.lib.min.js"] = b'{"message":"Not Found"}'
        self.assertEqual(self.run_update("--apply"), 2)
        Releases.assets["yt.solver.lib.min.js"] = b"  "
        self.assertEqual(self.run_update("--apply"), 2)
        self.assertEqual(self.source.read_text(encoding="utf-8"), SOURCE)

    def test_a_server_that_is_down_fails_and_changes_nothing(self):
        self.server.shutdown()
        self.server.server_close()
        self.assertEqual(self.run_update("--apply"), 2)
        self.assertEqual(self.source.read_text(encoding="utf-8"), SOURCE)

    def test_a_source_whose_layout_changed_is_reported_not_guessed(self):
        self.source.write_text(SOURCE.replace("SOLVER_CORE", "SOLVER_OTHER"), encoding="utf-8")
        self.assertEqual(self.run_update("--apply"), 2)

    def test_the_real_source_still_has_the_layout_the_script_reads(self):
        version, pairs, base, owner, repo = update.read_pin(update.SOURCE.read_text(encoding="utf-8"))
        self.assertRegex(version, r"^[0-9]+(\.[0-9]+)+$")
        self.assertEqual(sorted(pairs), ["SOLVER_CORE", "SOLVER_LIB"])
        for file, digest in pairs.values():
            self.assertTrue(file.endswith(".js"))
            self.assertRegex(digest, r"^[0-9a-f]{64}$")
        self.assertTrue(base.startswith("https://github.com/"))
        self.assertTrue(owner and repo)


if __name__ == "__main__":
    unittest.main()
