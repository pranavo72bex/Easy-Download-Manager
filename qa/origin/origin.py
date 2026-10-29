"""The QA origin server: every way a real server misbehaves, on request.

    GET /f/<name>?size=N&seed=S[&option...]     a file, its bytes from content.py
    GET /changing/<name>?size=N&flip=B          replaced after B bytes were served
    GET /redir/<code>/<n>/<rest>                n redirects with <code>, then /<rest>
    GET /loop/<code>/<name>                     redirects to itself for ever
    GET /rel/<n>/<rest>                         n relative redirects, then /<rest>
    GET /basic/<name>?...                       needs Basic qa:secret, else 401
    GET /cookie/<name>?...                      needs Cookie session=qa, else 403
    GET /stats  /reset                          what was asked, per name

Options on a file (query parameters):
    ranges=0      ignore Range, answer 200 with the whole body, no Accept-Ranges
    ar=none       send Accept-Ranges: none and ignore Range
    nolen=1       no Content-Length; the body ends when the connection closes
    chunked=1     Transfer-Encoding: chunked
    len=N         say Content-Length N whatever is sent
    drip=B        send B bytes a second
    stall=N       after N bytes of a response, send nothing more, for ever
                  (stalln=K: only the first K responses)
    cut=N&cutn=K  close the connection after N bytes, for the first K responses
    cd=V          Content-Disposition: V (URL-encoded in the query)
    etag=E lm=D   ETag / Last-Modified
    status=C      answer C with a short body
    e429=K&ra=S   the first K requests get 429 with Retry-After: S
    e503=K        the first K requests get 503
    delay=S       wait S seconds before answering
    ctype=T       Content-Type (default application/octet-stream)

Ports: 8080 HTTP; 8443 HTTPS with a certificate from a private CA; 8444 an
expired one; 8445 one for another host name; 8446 self-signed.
"""
import base64
import datetime
import http.server
import json
import os
import pathlib
import socketserver
import ssl
import sys
import threading
import time
import urllib.parse

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "lib"))
sys.path.insert(0, "/qa/lib")
import content  # noqa: E402

LOCK = threading.RLock()
STATS = {}  # name -> {"requests": n, "ranges": [...], "bytes": n, "headers": [...]}


def stat(name):
    with LOCK:
        return STATS.setdefault(name, {"requests": 0, "ranges": [], "bytes": 0, "open": 0, "max_open": 0})


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "qa-origin/1"

    def log_message(self, *a):
        pass

    # ---------------------------------------------------------------- helpers
    def q(self):
        return {k: v[-1] for k, v in urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query).items()}

    def small(self, code, body=b"", headers=()):
        self.send_response(code)
        for k, v in headers:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def do_HEAD(self):
        self.do_GET()

    def do_GET(self):
        try:
            self.route()
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
            pass

    def route(self):
        path = urllib.parse.urlsplit(self.path).path
        parts = [urllib.parse.unquote(p) for p in path.split("/") if p]
        if not parts:
            return self.small(200, b"qa-origin")
        head, rest = parts[0], parts[1:]
        if head == "stats":
            with LOCK:
                body = json.dumps(STATS).encode()
            return self.small(200, body, [("Content-Type", "application/json")])
        if head == "reset":
            with LOCK:
                STATS.clear()
            return self.small(200, b"ok")
        query = ("?" + urllib.parse.urlsplit(self.path).query) if urllib.parse.urlsplit(self.path).query else ""
        if head == "redir" and len(rest) >= 2:
            code, n = int(rest[0]), int(rest[1])
            target = "/" + "/".join(urllib.parse.quote(p) for p in rest[2:]) + query
            nxt = f"/redir/{code}/{n - 1}/" + "/".join(urllib.parse.quote(p) for p in rest[2:]) + query if n > 1 else target
            return self.small(code, b"", [("Location", nxt)])
        if head == "loop" and len(rest) >= 2:
            stat(rest[1])["requests"] += 1
            return self.small(int(rest[0]), b"", [("Location", self.path)])
        if head == "rel" and len(rest) >= 1:
            n = int(rest[0])
            tail = "/".join(urllib.parse.quote(p) for p in rest[1:]) + query
            # Relative to this path's folder (/rel/<n>/<tail folders>/): up to
            # /rel/ for the next hop, or up to / for the last.
            up = "../" * (len(rest) - 1)
            loc = f"{up}{n - 1}/{tail}" if n > 1 else f"{up}../{tail}"
            return self.small(302, b"", [("Location", loc)])
        if head == "basic" and rest:
            want = "Basic " + base64.b64encode(b"qa:secret").decode()
            if self.headers.get("Authorization") != want:
                return self.small(401, b"login needed", [("WWW-Authenticate", 'Basic realm="qa"')])
            return self.file(rest[-1], self.q())
        if head == "cookie" and rest:
            if "session=qa" not in (self.headers.get("Cookie") or ""):
                return self.small(403, b"no session")
            return self.file(rest[-1], self.q())
        if head == "changing" and rest:
            return self.changing(rest[-1], self.q())
        if head == "f" and rest:
            return self.file(rest[-1], self.q())
        return self.small(404, b"not found")

    # ---------------------------------------------------------------- files
    def changing(self, name, q):
        size = int(q.get("size", 4 << 20))
        flip = int(q.get("flip", size // 2))
        s = stat(name)
        with LOCK:
            second = s["bytes"] >= flip
        seed, etag = (hash_seed(name) + 1, '"v2"') if second else (hash_seed(name), '"v1"')
        q = dict(q, seed=str(seed), etag=etag, lm=("Wed, 02 Oct 2024 10:00:00 GMT" if second else "Tue, 01 Oct 2024 10:00:00 GMT"))
        if second and q.get("newsize"):
            q["size"] = q["newsize"]
        return self.file(name, q)

    def file(self, name, q):
        s = stat(name)
        with LOCK:
            s["requests"] += 1
            nth = s["requests"]
            s.setdefault("headers", []).append({k: v for k, v in self.headers.items() if k.lower() in ("range", "if-range", "cookie", "authorization", "referer", "user-agent")})
            s["headers"] = s["headers"][-50:]
        if q.get("delay"):
            time.sleep(float(q["delay"]))
        if int(q.get("e429", 0)) >= nth:
            return self.small(429, b"slow down", [("Retry-After", q.get("ra", "1"))])
        if int(q.get("e503", 0)) >= nth:
            return self.small(503, b"try later")
        if q.get("status"):
            return self.small(int(q["status"]), f"status {q['status']}".encode())

        size = int(q.get("size", 1 << 20))
        seed = int(q.get("seed", hash_seed(name)))
        honour = q.get("ranges", "1") != "0" and q.get("ar") != "none"
        start, end = 0, size
        rng = self.headers.get("Range")
        partial = False
        if honour and rng and rng.startswith("bytes="):
            a, _, b = rng[6:].split(",")[0].partition("-")
            if a == "":
                start = max(0, size - int(b))
            else:
                start = int(a)
                end = min(size, int(b) + 1) if b else size
            if_range = self.headers.get("If-Range")
            if if_range and q.get("etag") and if_range != q.get("etag"):
                start, end = 0, size  # a changed file: the whole of it, as RFC 9110 says
            elif start >= size or start >= end:
                return self.small(416, b"", [("Content-Range", f"bytes */{size}")])
            else:
                partial = True
        with LOCK:
            s["ranges"].append([start, end])
            s["ranges"] = s["ranges"][-200:]
            s["open"] += 1
            s["max_open"] = max(s["max_open"], s["open"])
        try:
            self.send_response(206 if partial else 200)
            self.send_header("Content-Type", q.get("ctype", "application/octet-stream"))
            if partial:
                self.send_header("Content-Range", f"bytes {start}-{end - 1}/{size}")
            if honour:
                self.send_header("Accept-Ranges", "bytes")
            elif q.get("ar") == "none":
                self.send_header("Accept-Ranges", "none")
            if q.get("etag"):
                self.send_header("ETag", q["etag"])
            if q.get("lm"):
                self.send_header("Last-Modified", q["lm"])
            if q.get("cd"):
                self.send_header("Content-Disposition", q["cd"])
            chunked = q.get("chunked") == "1"
            nolen = q.get("nolen") == "1"
            if chunked:
                self.send_header("Transfer-Encoding", "chunked")
            elif nolen:
                self.send_header("Connection", "close")
                self.close_connection = True
            else:
                self.send_header("Content-Length", q.get("len", str(end - start)))
                if q.get("len"):
                    self.send_header("Connection", "close")
                    self.close_connection = True
            self.end_headers()
            if self.command == "HEAD":
                return
            self.body(name, q, start, end, chunked, nth)
        finally:
            with LOCK:
                s["open"] -= 1

    def body(self, name, q, start, end, chunked, nth):
        drip = int(q.get("drip", 0))
        stall = int(q["stall"]) if "stall" in q and nth <= int(q.get("stalln", 1 << 30)) else None
        cut = int(q["cut"]) if "cut" in q and nth <= int(q.get("cutn", 1 << 30)) else None
        sent = 0
        for piece in content.chunks(int(q.get("seed", hash_seed(name))), start, end):
            for sub in split(piece, max(1, drip // 10) if drip else len(piece)):
                if stall is not None and sent + len(sub) > stall:
                    sub = sub[:max(0, stall - sent)]
                    self.emit(sub, chunked)
                    while True:  # hang, holding the connection open
                        time.sleep(3600)
                if cut is not None and sent + len(sub) > cut:
                    self.emit(sub[:max(0, cut - sent)], chunked)
                    self.wfile.flush()
                    self.close_connection = True
                    self.connection.shutdown(2)
                    return
                self.emit(sub, chunked)
                sent += len(sub)
                with LOCK:
                    stat(name)["bytes"] += len(sub)
                if drip:
                    time.sleep(len(sub) / drip)
        if chunked:
            self.wfile.write(b"0\r\n\r\n")

    def emit(self, data, chunked):
        if not data:
            return
        if chunked:
            self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
        else:
            self.wfile.write(data)


def split(data, n):
    for i in range(0, len(data), n):
        yield data[i:i + n]


def hash_seed(name):
    return sum(name.encode()) * 131 + len(name)


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True

    def handle_error(self, request, client_address):
        pass  # a client hanging up mid-body is what these tests do
    allow_reuse_address = True
    request_queue_size = 256


def certificates(folder):
    """A private CA and four server certificates, made at start."""
    from cryptography import x509
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import ec
    from cryptography.x509.oid import NameOID

    now = datetime.datetime.now(datetime.timezone.utc)
    day = datetime.timedelta(days=1)

    def key():
        return ec.generate_private_key(ec.SECP256R1())

    def name(cn):
        return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, cn)])

    ca_key = key()
    ca = (x509.CertificateBuilder().subject_name(name("qa private CA")).issuer_name(name("qa private CA"))
          .public_key(ca_key.public_key()).serial_number(x509.random_serial_number())
          .not_valid_before(now - day).not_valid_after(now + 30 * day)
          .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
          .sign(ca_key, hashes.SHA256()))

    def leaf(cn, hosts, before, after, signer=None, signer_key=None):
        k = key()
        b = (x509.CertificateBuilder().subject_name(name(cn))
             .issuer_name(signer.subject if signer else name(cn))
             .public_key(k.public_key()).serial_number(x509.random_serial_number())
             .not_valid_before(before).not_valid_after(after)
             .add_extension(x509.SubjectAlternativeName([x509.DNSName(h) for h in hosts]), critical=False))
        cert = b.sign(signer_key or k, hashes.SHA256())
        return cert, k

    made = {
        8443: leaf("origin", ["origin", "localhost"], now - day, now + 30 * day, ca, ca_key),
        8444: leaf("origin", ["origin", "localhost"], now - 30 * day, now - 2 * day, ca, ca_key),
        8445: leaf("other.example", ["other.example"], now - day, now + 30 * day, ca, ca_key),
        8446: leaf("origin", ["origin", "localhost"], now - day, now + 30 * day),
    }
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "ca.pem").write_bytes(ca.public_bytes(serialization.Encoding.PEM))
    out = {}
    for port, (cert, k) in made.items():
        c, kk = folder / f"{port}.pem", folder / f"{port}.key"
        c.write_bytes(cert.public_bytes(serialization.Encoding.PEM))
        kk.write_bytes(k.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
        out[port] = (c, kk)
    return out


def serve(port, tls=None):
    server = Server(("0.0.0.0", port), Handler)
    if tls:
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(*tls)
        server.socket = ctx.wrap_socket(server.socket, server_side=True)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


if __name__ == "__main__":
    serve(int(os.environ.get("PORT", 8080)))
    if os.environ.get("TLS", "1") == "1":
        for port, files in certificates(pathlib.Path(os.environ.get("CERTS", "/tmp/qa-certs"))).items():
            serve(port, tuple(map(str, files)))
    print("qa-origin ready", flush=True)
    while True:
        time.sleep(3600)
