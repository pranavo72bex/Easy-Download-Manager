// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// The line between a person and a server, for the benchmark's `wan` and `lossy` cases (gaps row
// W2): a TCP proxy in front of the harness's server that holds every piece for the one-way delay
// in each direction, and a server certificate made at run time for the cases served over TLS.
// Plain Node, no packages (AGENTS.md).
//
//   node bench/shaper.mjs --self-test     the delays measured through it, the bytes through it
//                                         whole, the certificate trusted by Node's own TLS
//
// What it does, and what it leaves to the server in bench.mjs:
// - Delay: each piece read on one side is written on the other `oneWayMs` later, in order (40 ms
//   each way is an 80 ms round trip). Timers on Windows fire up to ~15.6 ms late, so a piece
//   there can be held a little longer than asked; never less.
// - The handshake: the kernel accepts a loopback connection at once, where a real one costs a
//   round trip (SYN, SYN-ACK) before the client can send. So the first bytes a client sends on
//   a connection are held one round trip more. TLS's own round trip then costs what it costs,
//   because the TLS server sits behind the proxy and its handshake crosses the line.
// - Not here: the rate cap and the loss. Both are the sender's, in bench.mjs's server. A cap in
//   the proxy would leave megabytes in the loopback sockets' buffers between the server and the
//   proxy, where the harness cannot see them: the server would count bytes as sent (and its last
//   byte as gone) seconds before they reached the app. The 20 Mbit/s is the server's shared
//   token bucket, the one the capped baseline scenarios use. Loss is a stall at the sender: a
//   user-space proxy cannot drop a TCP segment (the kernel would only send it again at once), so
//   a lost piece is modelled as what TCP does about one, its connection sending nothing for about
//   one retransmission timeout before the piece goes again (bench.mjs `loss`).
import net from 'node:net';
import tls from 'node:tls';
import https from 'node:https';
import crypto from 'node:crypto';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';

// A lane holds no more than this before it stops reading its side (and the sender's writes wait).
const HIGH = 1024 * 1024;
const LOW = 256 * 1024;

// How much later than asked each piece was written, in 1 ms steps (the last step: 200 ms or more),
// for the line's own report: timers fire late, by up to a tick of the system's timer (15.6 ms on
// Windows) and more on a busy computer, and the results say by how much.
function lateness() {
  const steps = new Array(201).fill(0);
  let n = 0, sum = 0, max = 0;
  return {
    add(ms) { const v = Math.max(0, ms); n++; sum += v; max = Math.max(max, v); steps[Math.min(200, Math.floor(v))]++; },
    report() {
      let seen = 0, p95 = null;
      for (let i = 0; i < steps.length && p95 === null; i++) { seen += steps[i]; if (seen >= 0.95 * n) p95 = i; }
      return { pieces: n, meanMs: n ? +(sum / n).toFixed(1) : null, p95Ms: p95, maxMs: n ? +max.toFixed(1) : null };
    },
  };
}

// One direction of one connection: what `src` sends reaches `dst` `delayMs` later, the first
// piece `firstExtraMs` later still. `src` ending or closing reaches `dst` after the last piece.
function lane(src, dst, delayMs, firstExtraMs, late) {
  const q = [];
  let queued = 0, timer = null, lastAt = 0, first = true, paused = false, ended = false, closed = false;
  const backlog = () => queued + dst.writableLength;
  const loosen = () => { if (paused && backlog() < LOW) { paused = false; src.resume(); } };
  const arm = () => {
    if (timer || !q.length) return;
    timer = setTimeout(flush, Math.max(0, q[0].at - performance.now()));
  };
  const push = (item) => {
    const at = Math.max(performance.now() + delayMs + (first ? firstExtraMs : 0), lastAt);
    first = false; lastAt = at;
    q.push({ at, ...item });
    arm();
  };
  function flush() {
    timer = null;
    const now = performance.now();
    while (q.length && q[0].at <= now + 0.5) {
      const it = q.shift();
      if (dst.destroyed) { q.length = 0; break; }
      if (it.data) { queued -= it.data.length; late.add(now - it.at); dst.write(it.data); }
      else if (it.end) dst.end();
      else if (it.close) {
        // The other side went away without ending (a cut): this side is cut too, once what was
        // on the line has been written out. After an end, the end closes it.
        if (dst.writableLength) dst.end(() => dst.destroy()); else dst.destroy();
      }
    }
    loosen();
    arm();
  }
  src.on('data', (d) => {
    queued += d.length;
    push({ data: d });
    if (!paused && backlog() > HIGH) { paused = true; src.pause(); }
  });
  dst.on('drain', loosen);
  src.on('end', () => { ended = true; push({ end: true }); });
  src.on('close', () => { if (!closed && !ended) { closed = true; push({ close: true }); } });
}

// The proxy: listens on 127.0.0.1 and passes every connection to `target` across the line.
// Resolves with { port, connections(), late(), close() }: late() says how much later than asked
// the pieces were written.
export function shaper({ target, oneWayMs = 40, handshake = true, host = '127.0.0.1' }) {
  let count = 0;
  const late = lateness();
  const sockets = new Set();
  const server = net.createServer({ allowHalfOpen: true }, (client) => {
    count++;
    const up = net.connect({ host: target.host || '127.0.0.1', port: target.port, allowHalfOpen: true });
    for (const s of [client, up]) {
      s.setNoDelay(true);
      s.on('error', () => {});
      sockets.add(s);
      s.on('close', () => sockets.delete(s));
    }
    lane(client, up, oneWayMs, handshake ? 2 * oneWayMs : 0, late);
    lane(up, client, oneWayMs, 0, late);
  });
  return new Promise((resolve, reject) => {
    server.on('error', reject);
    server.listen(0, host, () => resolve({
      port: server.address().port,
      connections: () => count,
      late: () => late.report(),
      close: () => new Promise((r) => { for (const s of sockets) s.destroy(); server.close(() => r()); }),
    }));
  });
}

// ─── A server certificate for 127.0.0.1 and localhost, made with Node's crypto alone: an ECDSA
// P-256 key and a self-signed X.509 v3 certificate written out in DER by hand (Node signs and
// verifies certificates but has no call to make one). It is a server's certificate, not an
// authority's: basicConstraints says CA:FALSE, so trusting it trusts this certificate and no
// other it could sign. Resolves with { cert, key } as PEM.
const der = (tag, ...parts) => {
  const body = Buffer.concat(parts.map((p) => (Buffer.isBuffer(p) ? p : Buffer.from(p))));
  const n = body.length;
  const len = n < 128 ? [n] : n < 256 ? [0x81, n] : [0x82, n >> 8, n & 255];
  return Buffer.concat([Buffer.from([tag, ...len]), body]);
};
const seq = (...p) => der(0x30, ...p);
const oid = (...bytes) => der(0x06, bytes);
const utcTime = (d) => der(0x17, Buffer.from(d.toISOString().replace(/[-:T]/g, '').slice(2, 14) + 'Z'));

export function selfSignedCert({ days = 365, name = 'dm benchmark server' } = {}) {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ec', { namedCurve: 'prime256v1' });
  const serial = crypto.randomBytes(16);
  serial[0] = (serial[0] & 0x7f) || 1; // positive, and no leading zero byte
  const ecdsaSha256 = seq(oid(0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02));
  const dn = seq(der(0x31, seq(oid(0x55, 0x04, 0x03), der(0x0c, Buffer.from(name)))));
  const now = Date.now();
  const extension = (id, critical, value) => seq(id, ...(critical ? [der(0x01, [0xff])] : []), der(0x04, value));
  const extensions = der(0xa3, seq(
    // subjectAltName: DNS localhost, IP 127.0.0.1 (webpki matches names here only, never the CN).
    extension(oid(0x55, 0x1d, 0x11), false, seq(der(0x82, Buffer.from('localhost')), der(0x87, [127, 0, 0, 1]))),
    // basicConstraints: not an authority.
    extension(oid(0x55, 0x1d, 0x13), true, seq()),
    // extKeyUsage: serverAuth.
    extension(oid(0x55, 0x1d, 0x25), false, seq(oid(0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01))),
  ));
  const tbs = seq(
    der(0xa0, der(0x02, [2])), // v3
    der(0x02, serial),
    ecdsaSha256,
    dn,
    seq(utcTime(new Date(now - 3600e3)), utcTime(new Date(now + days * 86400e3))),
    dn,
    publicKey.export({ type: 'spki', format: 'der' }),
    extensions,
  );
  const signature = crypto.sign('sha256', tbs, privateKey); // DER ECDSA-Sig-Value
  const cert = seq(tbs, ecdsaSha256, der(0x03, Buffer.concat([Buffer.from([0]), signature])));
  const b64 = cert.toString('base64').match(/.{1,64}/g).join('\n');
  const label = 'CERTIFICATE';
  return { cert: `-----BEGIN ${label}-----\n${b64}\n-----END ${label}-----\n`, key: privateKey.export({ type: 'pkcs8', format: 'pem' }) };
}

// ─── Its own workings: a server behind the shaper, measured through it.
async function selfTest() {
  const ONE_WAY = 40;
  const fails = [];
  const expect = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) fails.push(what); };
  // 1. Round trips: an answering server; the first answer pays the handshake's round trip too.
  //    Each one-way hold may be as late as this computer's timers fire now (a tick of 15.6 ms on
  //    Windows, more when it is busy), and never earlier than asked. A busy computer's timers
  //    jump about, so up to three tries, each measuring the timers again; one must hold.
  const echo = net.createServer((s) => s.on('data', (d) => s.write(d)));
  await new Promise((r) => echo.listen(0, '127.0.0.1', r));
  let seen = [];
  for (let attempt = 1; attempt <= 3; attempt++) {
    const overshoot = [];
    for (let i = 0; i < 7; i++) { const t = performance.now(); await new Promise((r) => setTimeout(r, ONE_WAY)); overshoot.push(performance.now() - t - ONE_WAY); }
    const tick = overshoot.sort((a, b) => a - b)[3];
    const perHop = Math.max(5, 1.5 * tick + 3);
    const within = (ms, want) => ms >= want - 1 && ms <= want + 2 * perHop;
    const line = await shaper({ target: { port: echo.address().port }, oneWayMs: ONE_WAY });
    const c = net.connect(line.port, '127.0.0.1'); c.setNoDelay(true);
    await new Promise((r) => c.once('connect', r));
    const ask = () => new Promise((r) => { const t = performance.now(); c.once('data', () => r(performance.now() - t)); c.write('ping'); });
    const firstRtt = await ask(), second = await ask(), third = await ask();
    const late = line.late();
    c.destroy(); await line.close();
    seen = [
      [`timers here fire ${tick.toFixed(1)} ms late (median of 7): each one-way hold allowed ${perHop.toFixed(1)} ms over (try ${attempt})`, true],
      [`first answer after ${firstRtt.toFixed(1)} ms (handshake + one round trip: ${4 * ONE_WAY} ms)`, within(firstRtt, 4 * ONE_WAY)],
      [`then ${second.toFixed(1)} and ${third.toFixed(1)} ms (one round trip: ${2 * ONE_WAY} ms)`, within(second, 2 * ONE_WAY) && within(third, 2 * ONE_WAY)],
      [`the line says its pieces were written ${late.meanMs} ms late on average, at most ${late.maxMs}`, late.pieces === 6 && late.meanMs <= perHop && late.maxMs <= 2 * perHop],
    ];
    if (seen.every(([, ok]) => ok)) break;
  }
  for (const [what, ok] of seen) expect(ok, what);
  echo.close();

  // 2. Bytes: 16 MiB from a server through the line, whole and in order, at loopback speed less
  //    the delay (the line holds pieces, it does not cap them).
  const data = crypto.randomBytes(16 * 1024 * 1024);
  const want = crypto.createHash('sha256').update(data).digest('hex');
  const src = net.createServer((s) => { s.end(data); });
  await new Promise((r) => src.listen(0, '127.0.0.1', r));
  const line2 = await shaper({ target: { port: src.address().port }, oneWayMs: ONE_WAY });
  const t0 = performance.now();
  const got = await new Promise((r, j) => {
    const h = crypto.createHash('sha256'); let n = 0;
    const s = net.connect(line2.port, '127.0.0.1', () => s.write('go'));
    s.on('data', (d) => { h.update(d); n += d.length; });
    s.on('end', () => r({ n, sha: h.digest('hex'), ms: performance.now() - t0 }));
    s.on('error', j);
  });
  expect(got.n === data.length && got.sha === want, `16 MiB through the line whole (${got.n} bytes)`);
  expect(got.ms >= 3 * ONE_WAY, `the end reached the client after ${got.ms.toFixed(0)} ms, the line's delays included (at least ${3 * ONE_WAY} ms)`);
  expect(got.ms < 4000, `the line did not cap the bytes (16 MiB in ${(got.ms / 1000).toFixed(2)} s)`);
  await line2.close(); src.close();

  // 3. A cut on the server's side reaches the client as a cut, after what was on the line.
  const cutter = net.createServer((s) => { s.write('partial'); setTimeout(() => s.destroy(), 5); });
  await new Promise((r) => cutter.listen(0, '127.0.0.1', r));
  const line3 = await shaper({ target: { port: cutter.address().port }, oneWayMs: ONE_WAY });
  const cut = await new Promise((r) => {
    let said = ''; const s = net.connect(line3.port, '127.0.0.1', () => s.write('x'));
    s.on('data', (d) => { said += d; }); s.on('error', () => {}); s.on('close', () => r(said));
  });
  expect(cut === 'partial', `the bytes before a cut arrive, then the connection closes (${JSON.stringify(cut)})`);
  await line3.close(); cutter.close();

  // 4. The certificate: parsed, for 127.0.0.1 and localhost, trusted by Node's TLS as the only
  //    authority, across the line, and TLS's round trip seen.
  const made = selfSignedCert();
  const x = new crypto.X509Certificate(made.cert);
  expect(x.checkIP('127.0.0.1') === '127.0.0.1' && x.checkHost('localhost') === 'localhost' && !x.ca, 'the certificate is for 127.0.0.1 and localhost, and not an authority');
  expect(x.verify(x.publicKey), 'the certificate verifies with its own key');
  const web = https.createServer({ key: made.key, cert: made.cert }, (req, res) => res.end('hello over TLS'));
  await new Promise((r) => web.listen(0, '127.0.0.1', r));
  const line4 = await shaper({ target: { port: web.address().port }, oneWayMs: ONE_WAY });
  const t1 = performance.now();
  const said = await new Promise((r) => {
    https.get({ host: '127.0.0.1', port: line4.port, path: '/', ca: made.cert, agent: false }, (res) => {
      let b = ''; res.on('data', (d) => { b += d; }); res.on('end', () => r({ b, ms: performance.now() - t1 }));
    }).on('error', (e) => r({ b: `error: ${e.message}`, ms: 0 }));
  });
  expect(said.b === 'hello over TLS', `an HTTPS request through the line, the certificate trusted (${said.b})`);
  // Handshake 2, TLS 2, the request 2 (one-way delays).
  expect(said.ms >= 6 * ONE_WAY - 2, `the first HTTPS answer took ${said.ms.toFixed(0)} ms (connection, TLS and request: at least ${6 * ONE_WAY} ms)`);
  const refused = await new Promise((r) => {
    const s = tls.connect({ host: '127.0.0.1', port: line4.port, servername: 'localhost' }, () => { s.destroy(); r('accepted'); });
    s.on('error', (e) => r(e.code || e.message));
  });
  expect(refused !== 'accepted', `without it as an authority the certificate is refused (${refused})`);
  await line4.close(); web.close();

  if (fails.length) { console.error(`shaper.mjs self-test: ${fails.length} failed`); process.exit(1); }
  console.log('shaper.mjs self-test: ok');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.includes('--self-test')) await selfTest();
  else { console.error('usage: node bench/shaper.mjs --self-test (bench.mjs uses it for the wan and lossy cases)'); process.exit(2); }
}
