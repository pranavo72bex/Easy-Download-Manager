"""Serves the mock video site with ranges (yt-dlp and dm both ask for them)."""
import http.server
import os
import re
import socketserver

ROOT = "/site"


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k):
        super().__init__(*a, directory=ROOT, **k)

    def log_message(self, *a):
        pass

    def guess_type(self, path):
        if str(path).endswith("/watch"):
            return "text/html"
        return super().guess_type(path)

    def send_head(self):
        path = self.translate_path(self.path)
        rng = self.headers.get("Range")
        if not rng or not os.path.isfile(path):
            return super().send_head()
        m = re.match(r"bytes=(\d*)-(\d*)", rng)
        size = os.path.getsize(path)
        start = int(m.group(1) or 0)
        end = int(m.group(2)) + 1 if m.group(2) else size
        if start >= size:
            self.send_response(416)
            self.send_header("Content-Range", f"bytes */{size}")
            self.end_headers()
            return None
        f = open(path, "rb")
        f.seek(start)
        self.send_response(206)
        self.send_header("Content-Type", self.guess_type(path))
        self.send_header("Content-Range", f"bytes {start}-{end - 1}/{size}")
        self.send_header("Content-Length", str(end - start))
        self.send_header("Accept-Ranges", "bytes")
        self.end_headers()
        self.remaining = end - start
        return f

    def copyfile(self, source, outputfile):
        left = getattr(self, "remaining", None)
        if left is None:
            return super().copyfile(source, outputfile)
        while left > 0:
            data = source.read(min(65536, left))
            if not data:
                break
            outputfile.write(data)
            left -= len(data)


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


if __name__ == "__main__":
    print("qa-mockvideo ready", flush=True)
    Server(("0.0.0.0", 8090), Handler).serve_forever()
