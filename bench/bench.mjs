// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// The side-by-side benchmark. dm (and, on the Windows computer that has it, reference A) given
// the same address on the same local server, the file checked byte for byte when it lands, every
// parameter written down. dm's numbers are judged against reference A's frozen baseline by
// `compare.mjs`; `bench/README.md` says how, on every system.
//
//   node bench/bench.mjs [scenario ...]
//     scenarios: idle fast per_conn flaky outage batch big40 ceiling   (default: all of these)
//                inet (public internet, never a floor)   selftest (the server alone)
//
// The server, the bytes it serves and the scenarios below are the standard the baseline was
// measured with. Changing any of them makes dm's numbers incomparable with the baseline: add a
// new scenario instead.
//
// Environment (all optional):
//   DM_BINARY             dm to run (default: a release build under CARGO_TARGET_DIR or desktop/target)
//   BENCH_APPS            dm, or refa,dm (default: refa,dm on Windows when bench/local.json names it)
//   BENCH_OUT             results folder (default: bench/results/<system>-<date>)
//   BENCH_WORK            scratch folder (default: <temp>/dm-bench)
//   BENCH_DM_HOME         dm's home for the run (default: <work>/dm-home, wiped first)
//   BENCH_DM_DOWNLOADS    dm's download folder (default: <dm home>/downloads); use a fresh one per run
//   BENCH_DM_CONNECTIONS  dm's connections setting (default: dm's own; 16 for the 16-connection group)
//   BENCH_REFA_DIR        where reference A saves (default: <work>/refa)
//   BENCH_CEILING_DIR     where `ceiling` writes (default: dm's download folder)
//   BENCH_REPS            repetitions for fast, per_conn and flaky
//   BENCH_TAG             suffix for the result files' names
//   BENCH_CURL            curl for `selftest` (default: Windows' own on Windows)
//   BENCH_REFA_EXE, BENCH_REFA_PROCESS, BENCH_REFA_TITLE   reference A (or bench/local.json)
import http from 'node:http';
import net from 'node:net';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import readline from 'node:readline';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

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
// The window dm ships, on every system (docs/gaps.md G9).
const SHIPPED = 'dm-gpui';
function defaultDm() {
  for (const t of [process.env.CARGO_TARGET_DIR, path.join(REPO, 'desktop', 'target')].filter(Boolean)) {
    const p = path.join(t, 'release', SHIPPED + (WIN ? '.exe' : ''));
    if (fs.existsSync(p)) return p;
  }
  return null;
}
const DM = process.env.DM_BINARY || defaultDm();
const APPS = (process.env.BENCH_APPS || (WIN && REFA.exe ? 'refa,dm' : 'dm')).split(',').map((s) => s.trim()).filter(Boolean);
const WITH_REFA = APPS.includes('refa');
const WORK = process.env.BENCH_WORK || path.join(os.tmpdir(), 'dm-bench');
const OUT = process.env.BENCH_OUT || path.join(HERE, 'results', `${process.platform}-${new Date().toISOString().slice(0, 10)}`);
const MiB = 1024 * 1024;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const RUN = Date.now().toString(36);
// Windows' own curl: a curl found first on PATH can be a debug build (Git's ucrt64 one is), slow
// enough to make the ceiling the tool's and not the machine's.
const CURL = process.env.BENCH_CURL || (WIN && fs.existsSync(path.join(process.env.SystemRoot || 'C:\Windows', 'System32', 'curl.exe'))
  ? path.join(process.env.SystemRoot || 'C:\Windows', 'System32', 'curl.exe') : 'curl');

if (APPS.includes('dm') && (!DM || !fs.existsSync(DM))) {
  console.error(`No dm to run: set DM_BINARY, or build one (cd desktop && cargo build --release -p ${SHIPPED}).`);
  process.exit(2);
}
if (WITH_REFA && !(WIN && REFA.exe && REFA.process && REFA.title)) {
  console.error('Reference A runs only on Windows, named in bench/local.json (see bench/local.example.json).');
  process.exit(2);
}
if (DM && /[\\/]debug[\\/]/.test(DM)) console.warn(`warning: ${DM} is a debug build; the baseline is judged against a release build`);
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
    active: 0, maxActive: 0, activeFiles: new Map(), maxFiles: 0,
    bucket: { budget: 0, last: Date.now() }, cuts: prng(0xC0FFEE), outage: null, firstAfterOutage: null,
    timeline: [], // [ms since firstReq, bytes] every 250 ms
  };
  runs.set(key, run);
  return run;
}
const server = http.createServer((req, res) => {
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
  const entry = { t: now, method: req.method, range: req.headers.range || null, file: m[2], sent: 0 };
  run.requests.push(entry);
  res.writeHead(status, heads);
  if (req.method === 'HEAD') return res.end();
  run.sockets.add(sock);
  run.active++; run.maxActive = Math.max(run.maxActive, run.active);
  run.activeFiles.set(m[2], (run.activeFiles.get(m[2]) || 0) + 1);
  run.maxFiles = Math.max(run.maxFiles, run.activeFiles.size);
  const cutAfter = sc.cut ? Math.floor(sc.cut[0] + run.cuts() * (sc.cut[1] - sc.cut[0])) : Infinity;
  const conn = { budget: 0, last: Date.now() };
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
    if (sc.perConn) {
      conn.budget = Math.min(conn.budget + sc.perConn * (t - conn.last) / 1000, 128 * 1024); conn.last = t;
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
    if (sc.perConn) conn.budget -= k;
    if (sc.total) run.bucket.budget -= k;
    return k;
  };
  const send = () => {
    if (gone) return;
    if (at > b) return res.end();
    if (run.outage && Date.now() < run.outage.end) return sock.destroy();
    const want = Math.min(sc.perConn || sc.total ? 65536 : MiB, b - at + 1, cutAfter - entry.sent);
    if (want <= 0) return sock.destroy(); // this connection's cut
    const k = take(want);
    if (k === 0) return setTimeout(send, 4);
    const ok = res.write(file.data.subarray(at, at + k));
    at += k; entry.sent += k; run.served += k; run.lastByte = Date.now();
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
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const PORT = server.address().port;
setInterval(() => {
  for (const run of runs.values()) if (run.firstReq !== null && !run.closed) run.timeline.push([Date.now() - run.firstReq, run.served]);
}, 250).unref();

// ─── Process sampler, every 400 ms: memory (working set or resident; private or anonymous) and CPU
// time. Never blocking: the server runs in this process, and a blocked event loop stalls every
// connection of the app being measured.
const samples = []; // { t, app, pid, ws, priv, cpu } (bytes, bytes or null, ms)
const watched = new Map(); // pid -> app, on macOS and Linux
let sampler = null;
const appOfImage = (name) => (DM && name.toLowerCase() === path.parse(DM).name.toLowerCase() ? 'dm' : REFA.process && name.toLowerCase() === REFA.process.toLowerCase() ? 'refa' : null);
function startSampler() {
  if (WIN) {
    const names = [DM && path.parse(DM).name, WITH_REFA && REFA.process].filter(Boolean).map((n) => `'${n}'`).join(',');
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
    await sleep(100);
  }
  return null;
}

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

// ─── dm: its window shown, driven through its control port, a home of its own.
class Dm {
  constructor(home) { this.home = home; }
  async start() {
    fs.rmSync(this.home, { recursive: true, force: true });
    fs.mkdirSync(path.join(this.home, 'downloads'), { recursive: true });
    this.downloads = process.env.BENCH_DM_DOWNLOADS || path.join(this.home, 'downloads');
    fs.mkdirSync(this.downloads, { recursive: true });
    fs.writeFileSync(path.join(this.home, 'settings.json'), JSON.stringify({ download_dir: this.downloads, ...(process.env.BENCH_DM_CONNECTIONS ? { connections: Number(process.env.BENCH_DM_CONNECTIONS) } : {}) }));
    this.t0 = Date.now();
    this.proc = spawn(DM, ['--control', '0'], { env: { ...process.env, DM_HOME: this.home }, stdio: 'ignore' });
    watched.set(this.proc.pid, 'dm');
    const portFile = path.join(this.home, 'control.port');
    for (let i = 0; i < 400; i++) { if (fs.existsSync(portFile) && fs.readFileSync(portFile, 'utf8').trim()) break; await sleep(50); }
    if (!fs.existsSync(portFile)) throw new Error(`dm wrote no control port into ${this.home} within 20 s`);
    this.sock = net.connect(Number(fs.readFileSync(portFile, 'utf8')), '127.0.0.1');
    this.sock.on('error', () => {});
    this.buf = ''; this.waiters = [];
    this.sock.on('data', (d) => {
      this.buf += d; let i;
      while ((i = this.buf.indexOf('\n')) >= 0) { const l = this.buf.slice(0, i); this.buf = this.buf.slice(i + 1); const w = this.waiters.shift(); if (w) w(JSON.parse(l)); }
    });
    await new Promise((r) => this.sock.once('connect', r));
    await this.call('auth', { token: fs.readFileSync(path.join(this.home, 'control.token'), 'utf8').trim() });
    this.ready = Date.now();
  }
  call(method, params = {}) { return new Promise((r) => { this.waiters.push(r); this.sock.write(JSON.stringify({ id: 1, method, params }) + '\n'); }); }
  async rows() { return (await this.call('queue.list')).result || []; }
  stop() { try { this.proc.kill(); } catch {} }
}
let dm = null;
const dmHome = () => process.env.BENCH_DM_HOME || path.join(WORK, 'dm-home');
async function dmUp() { if (!dm) { dm = new Dm(dmHome()); await dm.start(); await sleep(2000); } return dm; }

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
    await sleep(100);
  }
  const t1 = mine.length === urls.length && mine.every((r) => r.state?.kind === 'completed') ? Date.now() : null;
  closerOff(); await sleep(1500); await closeRefa();
  return { t0, t1, files: mine.map((r) => r.path), states: mine.map((r) => r.state?.kind), errors: mine.map((r) => r.state?.message || r.state?.reason || null).filter(Boolean) };
}

// ─── What every result file says about where it was measured.
function meta() {
  let dmBinary = null;
  if (DM && fs.existsSync(DM)) { const s = fs.statSync(DM); dmBinary = { path: DM, bytes: s.size, modified: s.mtime.toISOString() }; }
  return {
    harness: 1, run: RUN, date: new Date().toISOString(), apps: APPS,
    system: { platform: process.platform, release: os.release(), arch: os.arch(), cpu: os.cpus()[0]?.model?.trim(), logicalCores: os.cpus().length, memoryGB: +(os.totalmem() / 2 ** 30).toFixed(1) },
    node: process.version, dmBinary,
    dmConnections: process.env.BENCH_DM_CONNECTIONS ? Number(process.env.BENCH_DM_CONNECTIONS) : null,
    folders: { work: WORK, dmHome: dmHome(), dmDownloads: process.env.BENCH_DM_DOWNLOADS || path.join(dmHome(), 'downloads'), refa: WITH_REFA ? (process.env.BENCH_REFA_DIR || path.join(WORK, 'refa')) : null },
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
  const urls = names.map((n) => `http://127.0.0.1:${PORT}/f/${key}/${n}`);
  const gate = () => run.firstReq === null;
  const r = app === 'refa'
    ? await runRefa(urls, names, sc.size, path.join(process.env.BENCH_REFA_DIR || path.join(WORK, 'refa'), key), sc.timeoutMs, sc.queue, gate)
    : await runDm(urls, names, sc.size, sc.timeoutMs, gate);
  run.closed = true;
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
    ...usage(app, r.t0, r.t1 || Date.now(), app === 'dm' ? dm?.proc?.pid ?? null : null),
    states: r.states, errors: r.errors, timeline: run.timeline.filter((_, i) => i % 2 === 0),
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
};
const DATA = {}, SHAS = {};

async function scenario(name) {
  const sc = SC[name];
  DATA[name] = []; SHAS[name] = [];
  for (let i = 0; i < (sc.files || 1); i++) { const d = makeData(sc.size, 1000 + i * 7 + name.length); DATA[name].push(d); SHAS[name].push(sha(d)); }
  const out = [];
  for (let rep = 1; rep <= sc.reps; rep++) {
    // Each repetition alternates which app goes first.
    const order = rep % 2 ? ['refa', 'dm'] : ['dm', 'refa'];
    for (const app of order.filter((a) => APPS.includes(a))) { out.push(await once(app, name, sc, rep)); await sleep(3000); }
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
  const dir = path.join(process.env.BENCH_CEILING_DIR || process.env.BENCH_DM_DOWNLOADS || path.join(WORK, 'ceiling'), `ceiling-${RUN}`);
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
      const order = rep % 2 ? ['refa', 'dm'] : ['dm', 'refa'];
      for (const app of order.filter((a) => APPS.includes(a))) {
        const r = app === 'refa'
          ? await runRefa([f.url + `?r=${rep}${RUN}`], [f.name], f.size, path.join(WORK, 'inet-refa', `${rep}-${RUN}-${f.name}`), 600000, false)
          : await runDm([f.url + `?r=${rep}${RUN}`], [f.name], f.size, 600000);
        const file = r.files[0];
        const digest = file && fs.existsSync(file) ? await shaFile(file) : null;
        const secs = r.t1 ? (r.t1 - r.t0) / 1000 : null;
        const res = { app, url: f.url, rep, ok: !!r.t1, seconds: secs, mbps: secs ? +(f.size / MiB / secs).toFixed(2) : null, sha256: digest,
          ...usage(app, r.t0, r.t1 || Date.now(), app === 'dm' ? dm?.proc?.pid ?? null : null), errors: r.errors };
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
  if (APPS.includes('dm')) await dmUp();
  await sleep(3000);
  const t0 = Date.now(); await sleep(20000); const t1 = Date.now();
  const out = APPS.map((app) => ({ app, scenario: 'idle', rep: 1, ok: true, ...usage(app, t0, t1, app === 'dm' ? dm?.proc?.pid ?? null : null) }));
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
  closerOff();
}

const asked = process.argv.slice(2);
const todo = asked.length ? asked : ['idle', 'fast', 'per_conn', 'flaky', 'outage', 'batch', 'big40', 'ceiling'];
for (const s of todo) if (!(s in SC) && !['inet', 'idle', 'selftest', 'ceiling'].includes(s)) { console.error(`unknown scenario ${s}`); process.exit(2); }
console.log(`# apps ${APPS.join(', ')}; dm ${DM || '-'}; results in ${OUT}`);
startSampler();
await sleep(1500);
try {
  for (const s of todo) {
    console.log(`# ${s}`);
    if (s === 'inet') await inet(); else if (s === 'idle') await idle(); else if (s === 'selftest') await selftest(); else if (s === 'ceiling') await ceiling(); else await scenario(s);
  }
} finally {
  if (dm) dm.stop();
  if (sampler) { if (WIN) sampler.kill(); else clearInterval(sampler); }
  server.close();
  setTimeout(() => process.exit(0), 500);
}
