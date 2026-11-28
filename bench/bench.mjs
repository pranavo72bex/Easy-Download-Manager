// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// The side-by-side benchmark. EDM (and, on the Windows computer that has it, reference A) given
// the same address on the same local server, the file checked byte for byte when it lands, every
// parameter written down. EDM's numbers are judged against reference A's frozen baseline by
// `compare.mjs`; `bench/README.md` says how, on every system.
//
//   node bench/bench.mjs [scenario ...]
//     scenarios: idle fast per_conn flaky outage batch big40 ceiling   (default: all of these)
//                wan lossy (a line between app and server, bench/shaper.mjs; asked by name: no
//                reference A baseline yet)
//                slowname (the file under a name whose every lookup takes 300 ms: asked by name,
//                no baseline yet, EDM alone unless the computer's DNS is pointed at the harness)
//                fast1g capped latency slowconn maxconns drops small256k, or all seven as refc
//                (reference C's own scenarios, gaps row C0: asked by name, seven runs each)
//                inet (public internet, never a floor)   selftest (the server alone)
//
// The server, the bytes it serves and the scenarios below are the standard the baseline was
// measured with. Changing any of them makes EDM's numbers incomparable with the baseline: add a
// new scenario instead.
//
// Environment (all optional):
//   EDM_BINARY             EDM to run (default: a release build under CARGO_TARGET_DIR or target)
//   BENCH_APPS            edm, or refa,edm (default: refa,edm on Windows when bench/local.json names it)
//   BENCH_OUT             results folder (default: bench/results/<system>-<date>)
//   BENCH_WORK            scratch folder (default: <temp>/edm-bench)
//   BENCH_EDM_HOME         EDM's home for the run (default: <work>/edm-home, wiped first)
//   BENCH_EDM_DOWNLOADS    EDM's download folder (default: <EDM home>/downloads); use a fresh one per run
//   BENCH_EDM_HEADLESS     1 runs EDM with no window (--headless): the window's share of the CPU,
//                         against a run without it (gaps row C0c)
//   BENCH_EDM_CONNECTIONS  EDM's connections setting (default: EDM's own; 16 for the 16-connection group)
//   BENCH_REFA_DIR        where reference A saves (default: <work>/refa)
//   BENCH_CEILING_DIR     where `ceiling` writes (default: EDM's download folder)
//   BENCH_REPS            repetitions for fast, per_conn and flaky
//   BENCH_REFC_REPS       repetitions for reference C's seven (default 7)
//   BENCH_TAG             suffix for the result files' names
//   BENCH_CURL            curl for `selftest` (default: Windows' own on Windows)
//   BENCH_WAN_TLS         0 serves `wan` over plain HTTP (EDM trusts the harness's certificate
//                         through EDM_BENCH_CA: bench/README.md)
//   BENCH_SLOWNAME_MS     how long each lookup of `slowname`'s name takes (default 300)
//   BENCH_SLOWNAME_PORT   the port its DNS server answers on, at 127.0.0.1 (default: any free one;
//                         53 where the computer is pointed at it)
//   BENCH_SLOWNAME_SYSTEM 1: the computer's own DNS sends names under bench.test to that server,
//                         so both apps look the name up through the system (bench/README.md)
//   BENCH_SLOWNAME_UPSTREAM  host:port the server passes every other name on to, for a computer
//                         pointed at it as its only DNS server
//   BENCH_REFA_EXE, BENCH_REFA_PROCESS, BENCH_REFA_TITLE   reference A (or bench/local.json)
import http from 'node:http';
import https from 'node:https';
import net from 'node:net';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import readline from 'node:readline';
import { spawn, execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import dns from 'node:dns';
import { shaper, selfSignedCert, slowDns } from './shaper.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '..');
const WIN = process.platform === 'win32';
const readJson = (f) => { try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; } };

// Reference A is named only in the git-ignored bench/local.json (or the environment): tracked
// files do not name it.
const local = readJson(path.join(HERE, 'local.json')) || {};
const REFA = {
  exe: process.env.BENCH_REFA_EXE || local.refa?.exe || null,
  process: process.env.BENCH_REFA_PROCESS || local.refa?.process || null,
  title: process.env.BENCH_REFA_TITLE || local.refa?.title || null,
};
// The window EDM ships, on every system (docs/gaps.md G9).
const SHIPPED = 'edm-gpui';
function defaultDm() {
  for (const t of [process.env.CARGO_TARGET_DIR, path.join(REPO, "target")].filter(Boolean)) {
    const p = path.join(t, 'release', SHIPPED + (WIN ? '.exe' : ''));
    if (fs.existsSync(p)) return p;
  }
  return null;
}
const EDM = process.env.EDM_BINARY || defaultDm();
const HEADLESS = process.env.BENCH_EDM_HEADLESS === '1';
const APPS = (process.env.BENCH_APPS || (WIN && REFA.exe ? 'refa,edm' : 'edm')).split(',').map((s) => s.trim()).filter(Boolean);
const WITH_REFA = APPS.includes('refa');
const WORK = process.env.BENCH_WORK || path.join(os.tmpdir(), 'edm-bench');
const OUT = process.env.BENCH_OUT || path.join(HERE, 'results', `${process.platform}-${new Date().toISOString().slice(0, 10)}`);
const MiB = 1024 * 1024;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const RUN = Date.now().toString(36);
// Windows' own curl: a curl found first on PATH can be a debug build (Git's ucrt64 one is), slow
// enough to make the ceiling the tool's and not the machine's.
const CURL = process.env.BENCH_CURL || (WIN && fs.existsSync(path.join(process.env.SystemRoot || 'C:\Windows', 'System32', 'curl.exe'))
  ? path.join(process.env.SystemRoot || 'C:\Windows', 'System32', 'curl.exe') : 'curl');

if (APPS.includes('edm') && (!EDM || !fs.existsSync(EDM))) {
  console.error(`No EDM to run: set EDM_BINARY, or build one cargo build --release -p ${SHIPPED}.`);
  process.exit(2);
}
if (WITH_REFA && !(WIN && REFA.exe && REFA.process && REFA.title)) {
  console.error('Reference A runs only on Windows, named in bench/local.json (see bench/local.example.json).');
  process.exit(2);
}
if (EDM && /[\\/]debug[\\/]/.test(EDM)) console.warn(`warning: ${EDM} is a debug build; the baseline is judged against a release build`);
fs.mkdirSync(OUT, { recursive: true });

// ─── The bytes served: a seeded 1 MiB block, each 4 KiB stamped with its MiB, so a piece put in
// the wrong place changes the file's SHA-256.
function makeData(size, seed) {
  let x = seed >>> 0 || 1;
  const block = Buffer.alloc(MiB);
  for (let i = 0; i < MiB; i += 4) { x ^= x << 13; x >>>= 0; x ^= x >>> 17; x ^= x << 5; x >>>= 0; block.writeUInt32LE(x, i); }
  const out = Buffer.allocUnsafe(size);
  for (let o = 0, m = 0; o < size; o += MiB, m++) {
    const n = Math.min(MiB, size - o);
    block.copy(out, o, 0, n);
    for (let s = 0; s + 4 <= n; s += 4096) out.writeUInt32LE((m ^ 0x9e3779b9) >>> 0, o + s);
  }
  return out;
}
const sha = (buf) => crypto.createHash('sha256').update(buf).digest('hex');
function shaFile(file) {
  return new Promise((r, j) => {
    const h = crypto.createHash('sha256');
    fs.createReadStream(file, { highWaterMark: 4 * MiB }).on('data', (d) => h.update(d)).on('end', () => r(h.digest('hex'))).on('error', j);
  });
}
function prng(seed) { let x = seed >>> 0 || 7; return () => { x ^= x << 13; x >>>= 0; x ^= x >>> 17; x ^= x << 5; x >>>= 0; return x / 4294967296; }; }

// ─── The server. Each run has its own path prefix and its own log.
const runs = new Map(); // key -> run
function newRun(key, sc, files) {
  const run = {
    key, sc, files, // files: name -> { data, sha }
    requests: [], conns: new Set(), sockets: new Set(), served: 0, firstReq: null, lastByte: null,
    active: 0, maxActive: 0, activeFiles: new Map(), maxFiles: 0, answers: 0,
    bucket: { budget: 0, last: Date.now() }, cuts: prng(0xC0FFEE), outage: null, firstAfterOutage: null,
    losses: prng(0x1055), stalls: 0, // `lossy` alone
    timeline: [], // [ms since firstReq, bytes] every 250 ms
  };
  runs.set(key, run);
  return run;
}
// The handler, the same for http (the baseline's) and for https (the `wan` case's, behind TLS).
const handle = (req, res) => {
  const m = /^\/f\/([^/]+)\/(.+)$/.exec(req.url);
  const run = m && runs.get(m[1]);
  const file = run && run.files.get(decodeURIComponent(m[2]));
  if (!file) { res.writeHead(404); return res.end(); }
  const sc = run.sc, now = Date.now();
  const sock = req.socket;
  run.conns.add(sock);
  if (run.firstReq === null) run.firstReq = now;
  // During an outage every request is dropped as it comes.
  if (run.outage && now < run.outage.end) { run.requests.push({ t: now, range: req.headers.range || null, dropped: true }); return sock.destroy(); }
  const size = file.data.length;
  const heads = { 'content-type': 'application/octet-stream', 'accept-ranges': 'bytes', etag: `"${file.sha.slice(0, 12)}"`,
    'last-modified': 'Mon, 01 Jan 2024 00:00:00 GMT' };
  let a = 0, b = size - 1, status = 200;
  const r = req.headers.range && /^bytes=(\d*)-(\d*)$/.exec(req.headers.range);
  if (r) {
    a = r[1] === '' ? Math.max(0, size - Number(r[2])) : Number(r[1]);
    b = r[1] !== '' && r[2] !== '' ? Math.min(Number(r[2]), size - 1) : size - 1;
    if (a >= size) { res.writeHead(416, { 'content-range': `bytes */${size}` }); return res.end(); }
    status = 206; heads['content-range'] = `bytes ${a}-${b}/${size}`;
  }
  heads['content-length'] = String(b - a + 1);
  // `maxconns`: past `sc.maxConns` answers being sent at once, a request is refused with 503, as a
  // server that allows so many connections a client does.
  if (sc.maxConns && req.method !== 'HEAD' && run.active >= sc.maxConns) {
    run.requests.push({ t: now, method: req.method, range: req.headers.range || null, file: m[2], sent: 0, refused: true });
    res.writeHead(503, { 'content-length': '0' });
    return res.end();
  }
  const entry = { t: now, method: req.method, range: req.headers.range || null, file: m[2], sent: 0 };
  run.requests.push(entry);
  res.writeHead(status, heads);
  if (req.method === 'HEAD') return res.end();
  run.sockets.add(sock);
  run.active++; run.maxActive = Math.max(run.maxActive, run.active);
  const nth = ++run.answers;
  run.activeFiles.set(m[2], (run.activeFiles.get(m[2]) || 0) + 1);
  run.maxFiles = Math.max(run.maxFiles, run.activeFiles.size);
  // `drops`: every `sc.cutEvery[0]`-th answer is cut after `sc.cutEvery[1]` bytes.
  const cutAfter = sc.cut ? Math.floor(sc.cut[0] + run.cuts() * (sc.cut[1] - sc.cut[0]))
    : sc.cutEvery && nth % sc.cutEvery[0] === 0 ? sc.cutEvery[1] : Infinity;
  // `slowconn`: every `sc.crawl[0]`-th answer crawls at `sc.crawl[1]` bytes a second, the others
  // at the scenario's own rate (none: as fast as they go).
  const rate = sc.crawl && nth % sc.crawl[0] === 0 ? sc.crawl[1] : sc.perConn;
  const conn = { budget: 0, last: Date.now(), pieceLeft: 0 };
  let at = a, gone = false, done = false;
  const finish = () => {
    if (done) return; done = true;
    run.active--; run.sockets.delete(sock);
    const k = (run.activeFiles.get(m[2]) || 1) - 1;
    if (k) run.activeFiles.set(m[2], k); else run.activeFiles.delete(m[2]);
  };
  res.on('close', () => { gone = true; finish(); });
  const take = (n) => {
    const t = Date.now();
    let k = n;
    if (rate) {
      conn.budget = Math.min(conn.budget + rate * (t - conn.last) / 1000, 128 * 1024); conn.last = t;
      k = Math.min(k, Math.floor(conn.budget));
    }
    if (sc.total) {
      const bk = run.bucket;
      bk.budget = Math.min(bk.budget + sc.total * (t - bk.last) / 1000, 256 * 1024); bk.last = t;
      k = Math.min(k, Math.floor(bk.budget));
    }
    // Send in pieces of at least 16 KiB (or what is left): a write of a few bytes each turn of the
    // loop makes the server, not the cap, the limit.
    if (k < Math.min(16384, n)) return 0;
    if (rate) conn.budget -= k;
    if (sc.total) run.bucket.budget -= k;
    return k;
  };
  const send = () => {
    if (gone) return;
    if (at > b) return res.end();
    if (run.outage && Date.now() < run.outage.end) return sock.destroy();
    let want = Math.min(rate || sc.total ? 65536 : MiB, b - at + 1, cutAfter - entry.sent);
    if (want <= 0) return sock.destroy(); // this connection's cut
    if (sc.loss) {
      // `lossy`: the bytes go in pieces of `sc.loss.piece`, and a lost piece is what TCP does
      // about one: its connection sends nothing for a retransmission timeout, then the piece goes
      // again. The others' share of the line is theirs meanwhile (bench/shaper.mjs says why the
      // loss is the sender's).
      if (conn.pieceLeft <= 0) {
        conn.pieceLeft = sc.loss.piece;
        if (run.losses() < sc.loss.p) { run.stalls++; return setTimeout(send, sc.loss.stallMs); }
      }
      want = Math.min(want, conn.pieceLeft);
    }
    // `tailstall`: the first answer to send once `at` of the file is served stops for `ms`.
    if (sc.tailStall && !run.tailStalled && entry.sent > 0 && run.served >= sc.tailStall.at * size) {
      run.tailStalled = true;
      return setTimeout(send, sc.tailStall.ms);
    }
    const k = take(want);
    if (k === 0) return setTimeout(send, 4);
    const ok = res.write(file.data.subarray(at, at + k));
    at += k; entry.sent += k; run.served += k; run.lastByte = Date.now();
    if (sc.loss) conn.pieceLeft -= k;
    if (run.outage && run.firstAfterOutage === null && Date.now() >= run.outage.end) run.firstAfterOutage = Date.now();
    // The outage starts once this share of the file has been sent.
    if (sc.outage && !run.outage && run.served >= sc.outage.at * size) {
      run.outage = { start: Date.now(), end: Date.now() + sc.outage.ms };
      for (const s of run.sockets) s.destroy();
      return;
    }
    if (ok) setImmediate(send); else res.once('drain', send);
  };
  send();
};
const server = http.createServer(handle);
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const PORT = server.address().port;

// ─── The `wan` and `lossy` cases' line (gaps row W2): bench/shaper.mjs in front of the server,
// one per run, and for `wan` the same handler behind TLS. The certificate is made once and kept
// in <work>/tls, so a computer told to trust it (reference A's: bench/README.md) keeps trusting
// it; EDM is given it as EDM_BENCH_CA.
const TLS_DIR = path.join(WORK, 'tls');
let tls = null, tlsServer = null;
function benchCert() {
  const certFile = path.join(TLS_DIR, 'bench-server.pem'), keyFile = path.join(TLS_DIR, 'bench-server.key');
  try {
    const cert = fs.readFileSync(certFile, 'utf8'), key = fs.readFileSync(keyFile, 'utf8');
    if (new Date(new crypto.X509Certificate(cert).validTo) - Date.now() > 7 * 86400e3) return { cert, key, certFile };
  } catch { /* none yet, or unreadable: made again */ }
  const made = selfSignedCert({ days: 365 });
  fs.mkdirSync(TLS_DIR, { recursive: true });
  fs.writeFileSync(certFile, made.cert);
  fs.writeFileSync(keyFile, made.key, { mode: 0o600 });
  return { ...made, certFile };
}
async function tlsPort() {
  if (!tlsServer) {
    tls ||= benchCert();
    tlsServer = https.createServer({ key: tls.key, cert: tls.cert }, handle);
    await new Promise((r) => tlsServer.listen(0, '127.0.0.1', r));
  }
  return tlsServer.address().port;
}
// Where one run's app is sent: { scheme, port, line } (line null for the baseline's scenarios).
async function lineFor(sc) {
  if (!sc.rttMs) return { scheme: 'http', port: PORT, line: null };
  const target = sc.tls ? await tlsPort() : PORT;
  const line = await shaper({ target: { port: target }, oneWayMs: sc.rttMs / 2 });
  return { scheme: sc.tls ? 'https' : 'http', port: line.port, line };
}
// ─── The `slowname` case's name (gaps row W2): a DNS server in this process (bench/shaper.mjs
// `slowDns`) that answers every name under bench.test with 127.0.0.1, each lookup BENCH_SLOWNAME_MS
// after it was asked, and counts the questions. EDM is told of it by EDM_BENCH_DNS, and asks it for
// names under .test alone. Reference A takes the system's resolver, which only a person can point
// here (BENCH_SLOWNAME_SYSTEM=1): until then the case runs for EDM alone.
const NAME_MS = Number(process.env.BENCH_SLOWNAME_MS || 300);
const REFC_REPS = Number(process.env.BENCH_REFC_REPS || 7);
const NAME_SYSTEM = process.env.BENCH_SLOWNAME_SYSTEM === '1';
let nameServer = null;
async function namesUp() {
  if (nameServer) return nameServer;
  nameServer = await slowDns({ delayMs: NAME_MS, port: Number(process.env.BENCH_SLOWNAME_PORT || 0), upstream: process.env.BENCH_SLOWNAME_UPSTREAM || null });
  console.log(`# slow name: *.bench.test answered after ${NAME_MS} ms at 127.0.0.1:${nameServer.port}${NAME_SYSTEM ? ' (the computer is pointed at it: both apps ask the system)' : ' (EDM is given it as EDM_BENCH_DNS)'}`);
  if (NAME_SYSTEM) {
    // The computer's own lookup, as reference A makes it (asynchronous: the server is this process).
    const probe = `probe-${RUN}.bench.test`, t = Date.now();
    const got = await dns.promises.lookup(probe, { family: 4 }).catch((e) => ({ address: e.code || e.message }));
    const ms = Date.now() - t;
    if (got.address !== '127.0.0.1' || !nameServer.asked(probe).a) {
      console.error(`BENCH_SLOWNAME_SYSTEM=1, but the computer did not ask 127.0.0.1:${nameServer.port} for ${probe} (${got.address}, ${ms} ms): point it at the server first, with BENCH_SLOWNAME_PORT=53 (bench/README.md)`);
      process.exit(2);
    }
    console.log(`# the computer's own lookup of ${probe} took ${ms} ms (asked ${NAME_MS})`);
  }
  return nameServer;
}
// Whether `app` runs case `sc`: reference A asks the system for a name, so it runs the slow name
// only where the computer is pointed at the harness's server.
const runsCase = (app, sc) => !(sc.nameMs && app === 'refa' && !NAME_SYSTEM);

setInterval(() => {
  for (const run of runs.values()) if (run.firstReq !== null && !run.closed) run.timeline.push([Date.now() - run.firstReq, run.served]);
}, 250).unref();

// ─── Process sampler, every 400 ms: memory (working set or resident; private or anonymous) and CPU
// time. Never blocking: the server runs in this process, and a blocked event loop stalls every
// connection of the app being measured.
const samples = []; // { t, app, pid, ws, priv, cpu } (bytes, bytes or null, ms)
const watched = new Map(); // pid -> app, on macOS and Linux
let sampler = null;
const appOfImage = (name) => (EDM && name.toLowerCase() === path.parse(EDM).name.toLowerCase() ? 'edm' : REFA.process && name.toLowerCase() === REFA.process.toLowerCase() ? 'refa' : null);
function startSampler() {
  if (WIN) {
    const names = [EDM && path.parse(EDM).name, WITH_REFA && REFA.process].filter(Boolean).map((n) => `'${n}'`).join(',');
    sampler = spawn('powershell', ['-NoProfile', '-Command',
      `while ($true) { $t = [DateTimeOffset]::Now.ToUnixTimeMilliseconds(); Get-Process -Name ${names} -ErrorAction SilentlyContinue | ForEach-Object { "$t,$($_.Name),$($_.Id),$($_.WorkingSet64),$($_.PrivateMemorySize64),$($_.TotalProcessorTime.TotalMilliseconds)" }; Start-Sleep -Milliseconds 400 }`],
      { stdio: ['ignore', 'pipe', 'ignore'] });
    readline.createInterface({ input: sampler.stdout }).on('line', (l) => {
      const [t, name, pid, ws, priv, cpu] = l.split(',');
      const app = appOfImage(name);
      if (app) samples.push({ t: +t, app, pid: +pid, ws: +ws, priv: +priv, cpu: +cpu });
    });
  } else if (process.platform === 'linux') {
    // /proc is read in microseconds; CPU in clock ticks of 10 ms (CLK_TCK 100).
    sampler = setInterval(() => {
      const t = Date.now();
      for (const [pid, app] of watched) {
        try {
          const status = fs.readFileSync(`/proc/${pid}/status`, 'utf8');
          const kb = (k) => { const m = new RegExp(`^${k}:\\s+(\\d+)`, 'm').exec(status); return m ? +m[1] * 1024 : null; };
          const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
          const f = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
          samples.push({ t, app, pid, ws: kb('VmRSS'), priv: kb('RssAnon'), cpu: (+f[11] + +f[12]) * 10 });
        } catch { /* gone */ }
      }
    }, 400);
  } else {
    sampler = setInterval(() => {
      if (!watched.size) return;
      const t = Date.now();
      let o = '';
      const c = spawn('ps', ['-o', 'pid=,rss=,time=', '-p', [...watched.keys()].join(',')], { stdio: ['ignore', 'pipe', 'ignore'] });
      c.stdout.on('data', (d) => { o += d; });
      c.on('exit', () => {
        for (const line of o.trim().split('\n')) {
          const [pid, rss, time] = line.trim().split(/\s+/);
          if (!watched.has(+pid)) continue;
          const secs = time.split(/[-:]/).reverse().reduce((s, v, i) => s + +v * [1, 60, 3600, 86400][i], 0);
          samples.push({ t, app: watched.get(+pid), pid: +pid, ws: +rss * 1024, priv: null, cpu: secs * 1000 });
        }
      });
    }, 400);
  }
}
function usage(app, t0, t1, pid = null) {
  const mine = samples.filter((s) => s.app === app && (pid === null || s.pid === pid));
  const within = mine.filter((s) => s.t >= t0 && s.t <= t1 + 400);
  const peak = within.reduce((p, s) => Math.max(p, s.ws || 0), 0);
  const peakPriv = within.some((s) => s.priv !== null) ? within.reduce((p, s) => Math.max(p, s.priv || 0), 0) : null;
  // CPU: each process's time at the last sample in the window less its time at the last before it.
  let cpu = 0;
  for (const id of new Set(within.map((s) => s.pid))) {
    const of = mine.filter((s) => s.pid === id);
    const before = of.filter((s) => s.t <= t0).at(-1) || of[0];
    const after = of.filter((s) => s.t <= t1 + 400).at(-1);
    if (before && after) cpu += Math.max(0, after.cpu - before.cpu);
  }
  return { peakWorkingSetMB: within.length ? +(peak / MiB).toFixed(1) : null, peakPrivateMB: peakPriv === null ? null : +(peakPriv / MiB).toFixed(1), cpuMs: within.length ? Math.round(cpu) : null };
}

// ─── Reference A (Windows only): its command line, its windows answered as a person answers them.
let closing = null; const closerRuns = [];
function closeRefa() {
  if (!WITH_REFA) return Promise.resolve();
  if (closing) return closing;
  closing = new Promise((r) => {
    const c = spawn('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(HERE, 'refa-windows.ps1'),
      '-Process', REFA.process, '-Title', REFA.title], { stdio: 'ignore' });
    c.on('exit', (code) => { closerRuns.push(code); closing = null; r(); }); c.on('error', () => { closing = null; r(); });
  });
  return closing;
}
let refaCloser = null;
// Only while `gate` holds (before the first request reaches the server): a closer running all
// through a download takes a core of a 2-core laptop from whichever app is measured.
function closerOn(gate = () => true) { if (WITH_REFA && !refaCloser) refaCloser = setInterval(() => { if (gate()) closeRefa(); }, 1000); }
function closerOff() { clearInterval(refaCloser); refaCloser = null; }
async function refaUp() {
  const out = await new Promise((r) => { let o = ''; const c = spawn('tasklist', ['/FI', `IMAGENAME eq ${REFA.process}.exe`, '/NH']); c.stdout.on('data', (d) => { o += d; }); c.on('exit', () => r(o)); });
  if (!out.toLowerCase().includes(`${REFA.process.toLowerCase()}.exe`)) { spawn(REFA.exe, ['/onboot'], { detached: true, stdio: 'ignore' }).unref(); await sleep(4000); }
}

// A finished file in `dir` at its full size.
async function whenWhole(dir, names, size, timeoutMs, t0) {
  const doneAt = new Map();
  while (Date.now() - t0 < timeoutMs) {
    for (const n of names) {
      if (doneAt.has(n)) continue;
      try { if (fs.statSync(path.join(dir, n)).size === size) doneAt.set(n, Date.now()); } catch {}
    }
    if (doneAt.size === names.length) return Math.max(...doneAt.values());
    await sleep(DONE_POLL_MS);
  }
  return null;
}

// How often a finished file is looked for: 100 ms, as reference A's runs were measured; less
// with BENCH_DONE_POLL_MS, to see a finish shorter than the look (gaps row 177).
const DONE_POLL_MS = Number(process.env.BENCH_DONE_POLL_MS) || 100;

async function runRefa(urls, names, size, dir, timeoutMs, queue, gate) {
  await refaUp();
  fs.rmSync(dir, { recursive: true, force: true }); fs.mkdirSync(dir, { recursive: true });
  await closeRefa();
  const t0 = Date.now();
  closerOn(gate || (() => Date.now() - t0 < 6000));
  if (queue) {
    // Each waited for without blocking, so the server keeps serving and the closer can answer.
    const ran = (args) => new Promise((r) => { const c = spawn(REFA.exe, args, { stdio: 'ignore' }); c.on('exit', r); c.on('error', r); });
    for (const u of urls) await ran(['/d', u, '/p', dir, '/n', '/a']);
    await ran(['/s']);
  } else {
    for (const u of urls) spawn(REFA.exe, ['/d', u, '/p', dir, '/n'], { detached: true, stdio: 'ignore' }).unref();
  }
  const t1 = await whenWhole(dir, names, size, timeoutMs, t0);
  closerOff(); await sleep(1500); await closeRefa();
  return { t0, t1, files: names.map((n) => path.join(dir, n)) };
}

// ─── EDM: its window shown, driven through its control port, a home of its own.
class Dm {
  constructor(home) { this.home = home; }
  async start() {
    fs.rmSync(this.home, { recursive: true, force: true });
    fs.mkdirSync(path.join(this.home, 'downloads'), { recursive: true });
    this.downloads = process.env.BENCH_EDM_DOWNLOADS || path.join(this.home, 'downloads');
    fs.mkdirSync(this.downloads, { recursive: true });
    fs.writeFileSync(path.join(this.home, 'settings.json'), JSON.stringify({ download_dir: this.downloads, ...(process.env.BENCH_EDM_CONNECTIONS ? { connections: Number(process.env.BENCH_EDM_CONNECTIONS) } : {}) }));
    this.t0 = Date.now();
    // EDM_BENCH_CA: the harness's certificate, which EDM trusts beside the public roots for the
    // `wan` case served over TLS (bench/README.md, "The line cases").
    // EDM_BENCH_DNS: the slow name's server, which EDM asks names under .test of (`slowname`),
    // unless the computer itself is pointed at it.
    const named = nameServer && !NAME_SYSTEM ? { EDM_BENCH_DNS: `127.0.0.1:${nameServer.port}` } : {};
    this.proc = spawn(EDM, HEADLESS ? ['--headless', '--control', '0'] : ['--control', '0'], { env: { ...process.env, EDM_HOME: this.home, ...(tls ? { EDM_BENCH_CA: tls.certFile } : {}), ...named }, stdio: 'ignore' });
    watched.set(this.proc.pid, 'edm');
    const portFile = path.join(this.home, 'control.port');
    for (let i = 0; i < 400; i++) { if (fs.existsSync(portFile) && fs.readFileSync(portFile, 'utf8').trim()) break; await sleep(50); }
    if (!fs.existsSync(portFile)) throw new Error(`EDM wrote no control port into ${this.home} within 20 s`);
    this.port = Number(fs.readFileSync(portFile, 'utf8'));
    this.token = fs.readFileSync(path.join(this.home, 'control.token'), 'utf8').trim();
    await this.connect();
    this.ready = Date.now();
  }
  // EDM cuts a control connection that says nothing for 300 s (`control.rs`), and reference A's
  // batch run lasts longer than that: the next request went into the cut socket, its error was
  // ignored, and the harness waited for an answer for ever (2026-11-13, Windows). A cut
  // connection is made again, and authenticated, before the next request.
  async connect() {
    this.sock = net.connect(this.port, '127.0.0.1');
    this.open = true;
    this.sock.on('error', () => {});
    this.sock.on('close', () => { this.open = false; });
    this.buf = ''; this.waiters = [];
    this.sock.on('data', (d) => {
      this.buf += d; let i;
      while ((i = this.buf.indexOf('\n')) >= 0) { const l = this.buf.slice(0, i); this.buf = this.buf.slice(i + 1); const w = this.waiters.shift(); if (w) w(JSON.parse(l)); }
    });
    await new Promise((r) => this.sock.once('connect', r));
    await this.send('auth', { token: this.token });
  }
  send(method, params = {}) { return new Promise((r) => { this.waiters.push(r); this.sock.write(JSON.stringify({ id: 1, method, params }) + '\n'); }); }
  async call(method, params = {}) {
    if (!this.open) await this.connect();
    return this.send(method, params);
  }
  async rows() { return (await this.call('queue.list')).result || []; }
  stop() { try { this.proc.kill(); } catch {} }
}
let edm = null;
const dmHome = () => process.env.BENCH_EDM_HOME || path.join(WORK, 'edm-home');
async function dmUp() { if (!edm) { edm = new Dm(dmHome()); await edm.start(); await sleep(2000); } return edm; }

async function runDm(urls, names, size, timeoutMs, gate) {
  const d = await dmUp();
  const before = new Set((await d.rows()).map((r) => r.id));
  await closeRefa();
  const t0 = Date.now();
  closerOn(gate || (() => Date.now() - t0 < 6000));
  await d.call('downloads.add', { urls });
  let mine = [];
  while (Date.now() - t0 < timeoutMs) {
    mine = (await d.rows()).filter((r) => !before.has(r.id));
    if (mine.length === urls.length && mine.every((r) => ['completed', 'failed', 'cancelled'].includes(r.state?.kind))) break;
    // As often as a finished file is looked for (DONE_POLL_MS): reference C's medians were timed
    // by a look every millisecond, so its seven are run with BENCH_DONE_POLL_MS=5.
    await sleep(DONE_POLL_MS);
  }
  const t1 = mine.length === urls.length && mine.every((r) => r.state?.kind === 'completed') ? Date.now() : null;
  closerOff(); await sleep(1500); await closeRefa();
  return { t0, t1, files: mine.map((r) => r.path), states: mine.map((r) => r.state?.kind), errors: mine.map((r) => r.state?.message || r.state?.reason || null).filter(Boolean) };
}

// ─── What every result file says about where it was measured.
// What built the binary and what drew it (gaps row 188), read once: the commit and whether it had
// changes not committed, rustc, the target and the profile (the folder cargo built it in), the GPU
// and each display's scale. A part the system will not say is null; nothing here fails a run.
let builtBy = null;
function provenance() {
  if (builtBy) return builtBy;
  const run = (cmd, args) => { try { return execFileSync(cmd, args, { encoding: 'utf8', timeout: 30000, stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch { return null; } };
  const rustc = run('rustc', ['-vV']) || run(path.join(os.homedir(), '.cargo', 'bin', 'rustc' + (WIN ? '.exe' : '')), ['-vV']);
  const field = (name) => rustc?.split('\n').find((l) => l.startsWith(name + ':'))?.slice(name.length + 1).trim() || null;
  const head = run('git', ['-C', REPO, 'rev-parse', 'HEAD']);
  const parts = EDM ? path.resolve(EDM).split(path.sep) : [];
  const at = parts.lastIndexOf('target');
  const inTarget = at >= 0 ? parts.slice(at + 1, -1) : [];
  let gpus = null, displays = null;
  if (process.platform === 'darwin') {
    try {
      const cards = JSON.parse(run('system_profiler', ['SPDisplaysDataType', '-json']) || '{}').SPDisplaysDataType || [];
      gpus = cards.map((c) => c.sppci_model || c._name);
      displays = cards.flatMap((c) => (c.spdisplays_ndrvs || []).map((d) => {
        const px = /(\d+) x (\d+)/.exec(d._spdisplays_pixels || ''), looks = /(\d+) x (\d+)/.exec(d._spdisplays_resolution || '');
        return { pixels: d._spdisplays_pixels || null, looksLike: d._spdisplays_resolution || null, scale: px && looks ? +(px[1] / looks[1]).toFixed(2) : null };
      }));
    } catch { /* left null */ }
  } else if (WIN) {
    gpus = (run('powershell', ['-NoProfile', '-Command', '(Get-CimInstance Win32_VideoController).Name']) || '').split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
    const dpi = Number(run('powershell', ['-NoProfile', '-Command', "(Get-ItemProperty 'HKCU:\\Control Panel\\Desktop\\WindowMetrics').AppliedDPI"]));
    displays = dpi ? [{ scale: +(dpi / 96).toFixed(2) }] : null;
  } else {
    gpus = (run('lspci', []) || '').split('\n').filter((l) => /VGA|3D controller|Display controller/.test(l)).map((l) => l.split(': ').pop());
  }
  builtBy = {
    commit: head ? { head, dirty: !!run('git', ['-C', REPO, 'status', '--porcelain', '--untracked-files=no']) } : null,
    rustc: rustc ? rustc.split('\n')[0] : null,
    target: inTarget.length === 2 ? inTarget[0] : field('host'),
    profile: inTarget.length ? inTarget[inTarget.length - 1] : null,
    gpus: gpus?.length ? gpus : null, displays: displays?.length ? displays : null,
  };
  return builtBy;
}
function meta() {
  let dmBinary = null;
  if (EDM && fs.existsSync(EDM)) { const s = fs.statSync(EDM); dmBinary = { path: EDM, bytes: s.size, modified: s.mtime.toISOString() }; }
  return {
    harness: 1, run: RUN, date: new Date().toISOString(), apps: APPS,
    system: { platform: process.platform, release: os.release(), arch: os.arch(), cpu: os.cpus()[0]?.model?.trim(), logicalCores: os.cpus().length, memoryGB: +(os.totalmem() / 2 ** 30).toFixed(1) },
    node: process.version, dmBinary, build: provenance(),
    dmConnections: process.env.BENCH_EDM_CONNECTIONS ? Number(process.env.BENCH_EDM_CONNECTIONS) : null,
    folders: { work: WORK, dmHome: dmHome(), dmDownloads: process.env.BENCH_EDM_DOWNLOADS || path.join(dmHome(), 'downloads'), refa: WITH_REFA ? (process.env.BENCH_REFA_DIR || path.join(WORK, 'refa')) : null },
  };
}
const write = (name, body) => fs.writeFileSync(path.join(OUT, `${name}${process.env.BENCH_TAG || ''}.json`), JSON.stringify({ ...body, meta: meta() }, null, 1));

// ─── One run of a scenario by one app.
async function once(app, name, sc, rep) {
  const key = `${name}-${app}-${rep}-${RUN}`;
  const files = new Map();
  const count = sc.files || 1;
  for (let i = 0; i < count; i++) files.set(`${name}-${app}-${rep}-${RUN}-${i + 1}.bin`, { data: DATA[name][i], sha: SHAS[name][i] });
  const run = newRun(key, sc, files);
  const names = [...files.keys()];
  const via = await lineFor(sc);
  // `slowname`: a name of the run's own under bench.test, so no app has it from a run before.
  const host = sc.nameMs ? `${key}.bench.test` : '127.0.0.1';
  const urls = names.map((n) => `${via.scheme}://${host}:${via.port}/f/${key}/${n}`);
  const gate = () => run.firstReq === null;
  const r = app === 'refa'
    ? await runRefa(urls, names, sc.size, path.join(process.env.BENCH_REFA_DIR || path.join(WORK, 'refa'), key), sc.timeoutMs, sc.queue, gate)
    : await runDm(urls, names, sc.size, sc.timeoutMs, gate);
  run.closed = true;
  const line = via.line ? { oneWayMs: sc.rttMs / 2, tls: !!sc.tls, late: via.line.late(), connections: via.line.connections(), lossStalls: sc.loss ? run.stalls : undefined } : undefined;
  // `slowname`: how often the app asked for its name (type A, and any other type), through what, and
  // the most an answer of its went later than asked (timers).
  const lookups = sc.nameMs ? { host, delayMs: sc.nameMs, through: NAME_SYSTEM ? 'system' : 'EDM_BENCH_DNS', ...nameServer.asked(host) } : undefined;
  if (via.line) await via.line.close();
  let whole = 0;
  for (let i = 0; i < names.length; i++) {
    const f = r.files[i];
    if (f && fs.existsSync(f) && (await shaFile(f)) === files.get(path.basename(f))?.sha) whole++;
  }
  const secs = r.t1 ? (r.t1 - r.t0) / 1000 : null;
  const res = {
    app, scenario: name, rep, ok: r.t1 !== null && whole === names.length,
    seconds: secs, mbps: secs ? +((sc.size * count) / MiB / secs).toFixed(2) : null,
    firstRequestMs: run.firstReq ? run.firstReq - r.t0 : null,
    lastByteToDoneMs: r.t1 && run.lastByte ? r.t1 - run.lastByte : null,
    filesWhole: whole, files: names.length,
    servedMB: +(run.served / MiB).toFixed(1), refetchedPct: +(100 * (run.served - sc.size * count) / (sc.size * count)).toFixed(1),
    requests: run.requests.length, connections: run.conns.size, maxParallelConnections: run.maxActive, maxParallelFiles: run.maxFiles,
    recoveryAfterOutageMs: run.outage && run.firstAfterOutage ? run.firstAfterOutage - run.outage.end : null,
    outageDroppedRequests: run.requests.filter((q) => q.dropped).length,
    outageAtMs: run.outage ? run.outage.start - run.firstReq : null,
    perRequest: run.requests.slice(0, 60).map((q) => [q.t - run.firstReq, q.sent ?? 0, q.range, q.dropped ? 'dropped' : '']),
    ...usage(app, r.t0, r.t1 || Date.now(), app === 'edm' ? edm?.proc?.pid ?? null : null),
    states: r.states, errors: r.errors, line, lookups, timeline: run.timeline.filter((_, i) => i % 2 === 0),
  };
  runs.delete(key);
  console.log(JSON.stringify({ ...res, timeline: undefined, perRequest: undefined }));
  return res;
}

// The scenarios of the baseline. Do not change one: add a new one.
const SC = {
  fast:     { size: 512 * MiB, label: '512 MiB, local server, no limit', reps: Number(process.env.BENCH_REPS || 3), timeoutMs: 300000 },
  per_conn: { size: 64 * MiB, perConn: 1 * MiB, label: '64 MiB, each connection 1 MiB/s', reps: Number(process.env.BENCH_REPS || 2), timeoutMs: 240000 },
  flaky:    { size: 64 * MiB, total: 8 * MiB, cut: [256 * 1024, 3 * MiB], label: '64 MiB at 8 MiB/s, every connection cut after 0.25-3 MiB', reps: Number(process.env.BENCH_REPS || 2), timeoutMs: 300000 },
  outage:   { size: 64 * MiB, total: 8 * MiB, outage: { at: 0.3, ms: 15000 }, label: '64 MiB at 8 MiB/s, all connections fail for 15 s at 30%', reps: 1, timeoutMs: 300000 },
  batch:    { size: 2 * MiB, files: 24, perConn: 2 * MiB, label: '24 files of 2 MiB, each connection 2 MiB/s, all added at once', reps: 2, timeoutMs: 300000 },
  big40:    { size: 512 * MiB, total: 40 * MiB, label: '512 MiB at 40 MiB/s in all', reps: 2, timeoutMs: 300000 },
  // Added after the baseline was frozen (gaps row W2): what a person's line adds. Reference A has
  // no numbers on these yet (compare.mjs: "no baseline yet"), so they are asked by name, not run by
  // default. The line is bench/shaper.mjs: 40 ms each way (an 80 ms round trip, a connection's
  // handshake included); the 20 Mbit/s (2,500,000 bytes a second) is the server's shared cap.
  wan:      { size: 32 * MiB, total: 2500000, rttMs: 80, tls: process.env.BENCH_WAN_TLS !== '0',
              label: `32 MiB over an 80 ms line at 20 Mbit/s in all, ${process.env.BENCH_WAN_TLS !== '0' ? 'over TLS' : 'plain HTTP (BENCH_WAN_TLS=0)'}`,
              reps: Number(process.env.BENCH_REPS || 2), timeoutMs: 240000 },
  // 1% loss, modelled as a stall: 1 in 100 pieces of 16 KiB stops its connection for 250 ms (about
  // one retransmission timeout at this round trip) before it goes again. Not packet loss.
  lossy:    { size: 32 * MiB, total: 2500000, rttMs: 80, loss: { p: 0.01, piece: 16384, stallMs: 250 },
              label: '32 MiB over an 80 ms line at 20 Mbit/s in all, 1% of 16 KiB pieces stalled 250 ms',
              reps: Number(process.env.BENCH_REPS || 2), timeoutMs: 300000 },
  // A name that resolves slowly: per_conn's file under a name of the run's own whose every lookup
  // takes BENCH_SLOWNAME_MS, and every connection cut after 1 to 4 MiB, so the download makes about
  // 25 connections. An app that keeps the name's answer pays the lookup once; one that asks again
  // for each connection pays it each time, its connection idle meanwhile. The server's answers have
  // a TTL of 0, so only an app's own keeping keeps them.
  slowname: { size: 64 * MiB, perConn: 1 * MiB, cut: [1 * MiB, 4 * MiB], nameMs: NAME_MS,
              label: `64 MiB under a name that takes ${NAME_MS} ms to resolve each time, each connection 1 MiB/s and cut after 1-4 MiB`,
              reps: Number(process.env.BENCH_REPS || 2), timeoutMs: 240000 },
  // Reference C's own seven (its docs/bench/run5/REPORT.md, gaps row C0), seven runs each as it
  // runs them, asked by name or all at once as `refc`. Its "MB/s" are taken as 1,000,000 bytes a
  // second and its "150 ms latency" as the round trip; no reference A baseline.
  fast1g:    { size: 1024 * MiB, label: '1 GiB over a fast local link', reps: REFC_REPS, timeoutMs: 300000 },
  capped:    { size: 100 * MiB, perConn: 2000000, label: '100 MiB, 2 MB/s per connection', reps: REFC_REPS, timeoutMs: 300000 },
  latency:   { size: 100 * MiB, perConn: 4000000, rttMs: 150, label: '100 MiB, 150 ms latency, 4 MB/s per connection', reps: REFC_REPS, timeoutMs: 300000 },
  slowconn:  { size: 32 * MiB, crawl: [3, 128000], label: '32 MiB, one connection in three crawls at 128 KB/s', reps: REFC_REPS, timeoutMs: 300000 },
  maxconns:  { size: 50 * MiB, perConn: 2000000, maxConns: 2, label: '50 MiB, the server allows 2 connections (503 past them), 2 MB/s each', reps: REFC_REPS, timeoutMs: 300000 },
  drops:     { size: 50 * MiB, perConn: 4000000, cutEvery: [2, 2 * MiB], label: '50 MiB, every 2nd connection cut after 2 MiB, 4 MB/s each', reps: REFC_REPS, timeoutMs: 300000 },
  // Added for gaps row K1, EDM alone, no baseline: once nine tenths of the file is served, one
  // answer under way stops sending for 20 s, its connection left open, as a server that stalls one
  // range near the end. What it measures is how long the others wait on it.
  tailstall: { size: 64 * MiB, perConn: 2 * MiB, tailStall: { at: 0.9, ms: 20000 }, label: '64 MiB, each connection 2 MiB/s, one answer stalled 20 s once 90% is served', reps: Number(process.env.BENCH_REPS || 3), timeoutMs: 120000 },
  small256k: { size: 256 * 1024, label: '256 KiB file over a fast local link', reps: REFC_REPS, timeoutMs: 60000 },
};
const REFC = ['fast1g', 'capped', 'latency', 'slowconn', 'maxconns', 'drops', 'small256k'];
const DATA = {}, SHAS = {};

async function scenario(name) {
  const sc = SC[name];
  // Reference C's seven each with an EDM of its own, as its own were each run in a process of their
  // own: one server serves them all, and what EDM learnt of it in one (maxconns' two at once, kept
  // for the rest of a run by `agents::remember_ceiling`) is not carried into the next.
  if (REFC.includes(name) && edm) { edm.stop(); edm = null; await sleep(1500); }
  if (WITH_REFA && !runsCase('refa', sc)) console.log(`# ${name}: EDM alone; reference A looks names up through the system, which is not pointed at the slow name (BENCH_SLOWNAME_SYSTEM, bench/README.md)`);
  DATA[name] = []; SHAS[name] = [];
  for (let i = 0; i < (sc.files || 1); i++) { const d = makeData(sc.size, 1000 + i * 7 + name.length); DATA[name].push(d); SHAS[name].push(sha(d)); }
  const out = [];
  for (let rep = 1; rep <= sc.reps; rep++) {
    // Each repetition alternates which app goes first.
    const order = rep % 2 ? ['refa', 'edm'] : ['edm', 'refa'];
    for (const app of order.filter((a) => APPS.includes(a) && runsCase(a, sc))) { out.push(await once(app, name, sc, rep)); await sleep(3000); }
  }
  delete DATA[name];
  const { reps, timeoutMs, label, ...params } = sc;
  write(name, { scenario: name, label, params, size: sc.size, files: sc.files || 1, runs: out });
}

// ─── The machine's own ceiling: the reference client (`ceiling-client.mjs`, a process of its own)
// fetching eight ranges at once, each written as a file into the download folder, under the
// scenario's caps (no cuts, no outage). It is how `compare.mjs` carries the machine-bound floors
// (fast, big40) to another computer. Not curl: on Windows its small writes made the disk, not the
// machine, the limit (512 MiB in 21.6 s where reference A took 3.7).
async function ceiling() {
  const dir = path.join(process.env.BENCH_CEILING_DIR || process.env.BENCH_EDM_DOWNLOADS || path.join(WORK, 'ceiling'), `ceiling-${RUN}`);
  const out = {};
  for (const name of ['fast', 'per_conn', 'flaky', 'big40']) {
    const { size, perConn, total } = SC[name];
    const sc = { size, perConn, total };
    const data = makeData(size, 5);
    const times = [];
    for (let rep = 1; rep <= 3; rep++) {
      const key = `ceiling-${name}-${rep}-${RUN}`;
      newRun(key, sc, new Map([['x.bin', { data, sha: sha(data) }]]));
      fs.rmSync(dir, { recursive: true, force: true }); fs.mkdirSync(dir, { recursive: true });
      const said = await new Promise((r) => {
        let o = '', e = '';
        const c = spawn(process.execPath, [path.join(HERE, 'ceiling-client.mjs'), `http://127.0.0.1:${PORT}/f/${key}/x.bin`, String(size), '8', dir], { stdio: ['ignore', 'pipe', 'pipe'] });
        c.stdout.on('data', (d) => { o += d; }); c.stderr.on('data', (d) => { e += d; });
        c.on('exit', (code) => r({ code, o, e })); c.on('error', (err) => r({ code: -1, o, e: err.message }));
      });
      runs.delete(key);
      if (said.code !== 0) throw new Error(`ceiling ${name}: ${said.e.trim() || `exit ${said.code}`}`);
      const secs = Number(said.o.trim());
      times.push(secs);
      console.log(`ceiling ${name} rep ${rep}: ${secs.toFixed(3)} s`);
      fs.rmSync(dir, { recursive: true, force: true });
      await sleep(1500);
    }
    out[name] = { seconds: times, best: Math.min(...times) };
  }
  write('ceiling', { scenario: 'ceiling', client: 'bench/ceiling-client.mjs', connections: 8, dir, results: out });
}

// ─── The real internet: the same public file by each app, twice, in turn. Never a floor: the
// network is not the same twice.
const INET = [
  { url: 'http://ipv4.download.thinkbroadband.com/100MB.zip', name: '100MB.zip', size: 104857600 },
  { url: 'https://proof.ovh.net/files/100Mb.dat', name: '100Mb.dat', size: 104857600 },
];
async function inet() {
  const out = [];
  for (const f of INET) {
    for (let rep = 1; rep <= 2; rep++) {
      const order = rep % 2 ? ['refa', 'edm'] : ['edm', 'refa'];
      for (const app of order.filter((a) => APPS.includes(a))) {
        const r = app === 'refa'
          ? await runRefa([f.url + `?r=${rep}${RUN}`], [f.name], f.size, path.join(WORK, 'inet-refa', `${rep}-${RUN}-${f.name}`), 600000, false)
          : await runDm([f.url + `?r=${rep}${RUN}`], [f.name], f.size, 600000);
        const file = r.files[0];
        const digest = file && fs.existsSync(file) ? await shaFile(file) : null;
        const secs = r.t1 ? (r.t1 - r.t0) / 1000 : null;
        const res = { app, url: f.url, rep, ok: !!r.t1, seconds: secs, mbps: secs ? +(f.size / MiB / secs).toFixed(2) : null, sha256: digest,
          ...usage(app, r.t0, r.t1 || Date.now(), app === 'edm' ? edm?.proc?.pid ?? null : null), errors: r.errors };
        console.log(JSON.stringify(res));
        out.push(res);
        if (file && fs.existsSync(file)) fs.rmSync(file, { force: true });
        await sleep(3000);
      }
    }
  }
  write('inet', { scenario: 'inet', runs: out });
}

// ─── Idle: each app with its window up and nothing to do, for 20 s.
async function idle() {
  if (WITH_REFA) { await refaUp(); spawn(REFA.exe, [], { detached: true, stdio: 'ignore' }).unref(); }
  if (APPS.includes('edm')) await dmUp();
  await sleep(3000);
  const t0 = Date.now(); await sleep(20000); const t1 = Date.now();
  const out = APPS.map((app) => ({ app, scenario: 'idle', rep: 1, ok: true, ...usage(app, t0, t1, app === 'edm' ? edm?.proc?.pid ?? null : null) }));
  console.log(JSON.stringify(out));
  write('idle', { scenario: 'idle', label: 'window up, nothing to do, 20 s', runs: out });
}

// ─── The server checked alone: 8 curl connections, each an eighth of 16 MiB, under each cap.
async function selftest() {
  closerOn();
  for (const [label, sc] of [['per_conn 1 MiB/s x 8', { size: 16 * MiB, perConn: MiB }], ['total 8 MiB/s', { size: 16 * MiB, total: 8 * MiB }], ['no cap', { size: 16 * MiB }]]) {
    const data = makeData(sc.size, 5); const key = `self-${label.replace(/\W/g, '')}-${RUN}`;
    newRun(key, sc, new Map([['x.bin', { data, sha: sha(data) }]]));
    const t0 = Date.now(); const part = sc.size / 8;
    await Promise.all([...Array(8).keys()].map((i) => new Promise((r) => {
      const c = spawn(CURL, ['-s', '-o', WIN ? 'NUL' : '/dev/null', '-r', `${i * part}-${(i + 1) * part - 1}`, `http://127.0.0.1:${PORT}/f/${key}/x.bin`]); c.on('exit', r); c.on('error', r); })));
    if (WITH_REFA) console.log('closer runs so far (exit codes):', closerRuns.join(','));
    console.log(label, ((Date.now() - t0) / 1000).toFixed(2), 's for 16 MiB =', (16 / ((Date.now() - t0) / 1000)).toFixed(2), 'MiB/s');
  }
  // The lines of `wan` and `lossy`, through the shaper (and TLS): the first byte after the
  // connection's, TLS's and the request's round trips (240 ms over TLS, 160 without), and 8 ranges
  // of 8 MiB at the 20 Mbit/s cap, with lossy's stalls counted (about 1 in 100 of 512 pieces).
  for (const [label, sc] of [['wan', { size: 8 * MiB, total: 2500000, rttMs: 80, tls: true }],
    ['lossy', { size: 8 * MiB, total: 2500000, rttMs: 80, loss: SC.lossy.loss }]]) {
    const data = makeData(sc.size, 5); const key = `self-${label}-${RUN}`;
    const run = newRun(key, sc, new Map([['x.bin', { data, sha: sha(data) }]]));
    const via = await lineFor(sc);
    const url = `${via.scheme}://127.0.0.1:${via.port}/f/${key}/x.bin`;
    const curl = (args) => new Promise((r) => {
      let o = '';
      const c = spawn(CURL, ['-s', '-k', '-o', WIN ? 'NUL' : '/dev/null', ...args, url], { stdio: ['ignore', 'pipe', 'ignore'] });
      c.stdout.on('data', (d) => { o += d; }); c.on('exit', () => r(o)); c.on('error', () => r(o));
    });
    const said = (await curl(['-r', '0-0', '-w', '%{time_appconnect} %{time_starttransfer}'])).trim().split(' ').map(Number);
    console.log(`${label} line: ${sc.tls ? `TLS up after ${(said[0] * 1000).toFixed(0)} ms (asked 160), ` : ''}first byte after ${(said[1] * 1000).toFixed(0)} ms (asked ${sc.tls ? 240 : 160})`);
    const t0 = Date.now(); const part = sc.size / 8;
    await Promise.all([...Array(8).keys()].map((i) => curl(['-r', `${i * part}-${(i + 1) * part - 1}`])));
    const secs = (Date.now() - t0) / 1000;
    console.log(`${label} line: 8 MiB in ${secs.toFixed(2)} s = ${(sc.size / secs * 8 / 1e6).toFixed(1)} Mbit/s (cap 20: ${(sc.size / sc.total).toFixed(2)} s at the cap, plus the round trips)${sc.loss ? `; ${run.stalls} stalls of ${sc.loss.stallMs} ms` : ''}`);
    console.log(`${label} line: pieces written late by`, JSON.stringify(via.line.late()));
    await via.line.close();
    runs.delete(key);
  }
  // The slow name: Node's own resolver asks its server (as EDM does, through EDM_BENCH_DNS) for the
  // same name twice, each answer no sooner than the delay.
  const ns = await namesUp();
  const resolver = new dns.promises.Resolver({ timeout: 3000, tries: 1 });
  resolver.setServers([`127.0.0.1:${ns.port}`]);
  for (const i of [1, 2]) {
    const t = Date.now();
    const got = await resolver.resolve4(`self-${RUN}.bench.test`).catch((e) => [e.code || e.message]);
    console.log(`slow name: lookup ${i} of self-${RUN}.bench.test gave ${got.join(', ')} after ${Date.now() - t} ms (asked ${NAME_MS})`);
  }
  console.log('slow name: questions counted', JSON.stringify(ns.asked(`self-${RUN}.bench.test`)), '; answers sent late by', JSON.stringify(ns.late()));
  closerOff();
}

const asked = process.argv.slice(2).flatMap((s) => (s === 'refc' ? REFC : [s]));
const todo = asked.length ? asked : ['idle', 'fast', 'per_conn', 'flaky', 'outage', 'batch', 'big40', 'ceiling'];
for (const s of todo) if (!(s in SC) && !['inet', 'idle', 'selftest', 'ceiling'].includes(s)) { console.error(`unknown scenario ${s}`); process.exit(2); }
console.log(`# apps ${APPS.join(', ')}; EDM ${EDM || '-'}; results in ${OUT}`);
// The certificate before EDM starts, whichever scenario starts it: EDM is given it once, at start.
if (todo.some((s) => SC[s]?.tls)) { tls = benchCert(); console.log(`# TLS certificate ${tls.certFile} (EDM is given it as EDM_BENCH_CA)`); }
// The slow name's server before EDM starts, likewise: EDM is given it once, at start.
if (todo.some((s) => SC[s]?.nameMs)) await namesUp();
startSampler();
await sleep(1500);
try {
  for (const s of todo) {
    console.log(`# ${s}`);
    if (s === 'inet') await inet(); else if (s === 'idle') await idle(); else if (s === 'selftest') await selftest(); else if (s === 'ceiling') await ceiling(); else await scenario(s);
  }
} finally {
  if (edm) edm.stop();
  if (sampler) { if (WIN) sampler.kill(); else clearInterval(sampler); }
  server.close();
  if (tlsServer) tlsServer.close();
  if (nameServer) nameServer.close();
  setTimeout(() => process.exit(0), 500);
}
