// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// EDM against its own last numbers, parameter by parameter (gaps row R5). `compare.mjs` judges EDM
// against reference A's frozen floor; this judges a run of `bench.mjs` against an earlier run of
// EDM on the same kind of computer (CI's weekly benchmark, .github/workflows/bench.yml), so a change
// that makes EDM slower or heavier is seen the week it lands, wherever reference A's floor is still
// far away.
//
//   node bench/against-last.mjs <this run's results> <last run's results> [--tolerance <percent>]
//                               [--markdown <file>] [--json]
//   node bench/against-last.mjs --self-test
//
// Each parameter compares EDM's median over its whole runs with the last run's median. It is worse
// when it is over the last one by more than the tolerance (default 20%) and a small allowance for
// what the harness can see (`SLACK`: the finish is looked for every 100 ms, CPU is read in 10 ms
// ticks). The machine-bound scenarios' times (`fast`, `big40`: seconds, transferSeconds,
// lastByteToDoneMs) are scaled as bench/README.md scales the frozen floors, by the ceiling each run
// measured on its own computer: the last number x (ceiling now / ceiling then). Unlike the frozen
// floor's min(1, ...), the factor goes both ways: the last run is a number on another runner, not a
// bar that may only get stricter, and a slower runner this week is not EDM getting slower.
//
// It reads bench/reference-a-baseline.json for the parameters' names, units and directions and
// for which scenarios are machine-bound; it never writes it, and never judges against its floors.
// Exit status: 0 no parameter worse than the last run beyond the tolerance; 1 worse somewhere, a
// file not whole, or a scenario the last run had and this one has not; 2 nothing to compare.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const B = JSON.parse(fs.readFileSync(path.join(HERE, 'reference-a-baseline.json'), 'utf8'));
const ORDER = ['idle', 'fast', 'per_conn', 'flaky', 'outage', 'batch', 'big40', 'wan', 'lossy', 'slowname'];
// What the harness cannot tell apart, by unit, added to the tolerance.
const SLACK = { s: 0.25, ms: 150, '%': 0.5, MiB: 3 };
const median = (xs) => { const v = xs.filter((x) => x != null).sort((a, b) => a - b); if (!v.length) return null; const m = v.length >> 1; return v.length % 2 ? v[m] : (v[m - 1] + v[m]) / 2; };

// A results folder (or files): EDM's runs by scenario, the ceiling, and where it ran.
export function load(inputs) {
  const files = inputs.flatMap((p) => (fs.statSync(p).isDirectory() ? fs.readdirSync(p).filter((f) => f.endsWith('.json')).map((f) => path.join(p, f)) : [p]));
  const out = { scenarios: new Map(), ceiling: null, meta: null };
  for (const f of files) {
    let j; try { j = JSON.parse(fs.readFileSync(f, 'utf8')); } catch { continue; }
    if (j.scenario === 'ceiling') { out.ceiling = j.results || null; continue; }
    if (!Array.isArray(j.runs) || !j.scenario || j.scenario === 'inet') continue;
    out.meta ||= j.meta || null;
    const edm = j.runs.filter((r) => (r.app === 'edm' || r.app === 'dm')).map((r) => ({ ...r, transferSeconds: r.seconds != null && r.firstRequestMs != null ? r.seconds - r.firstRequestMs / 1000 : null }));
    out.scenarios.set(j.scenario, [...(out.scenarios.get(j.scenario) || []), ...edm]);
  }
  return out;
}

export function judge(now, last, tolerance = 20) {
  const report = { tolerance, slack: SLACK, scenarios: [], worse: [], notCompared: [] };
  const names = [...new Set([...now.scenarios.keys(), ...last.scenarios.keys()])].sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
  for (const sc of names) {
    const spec = B.scenarios[sc] || {};
    const runs = now.scenarios.get(sc), before = last.scenarios.get(sc);
    const out = { scenario: sc, label: spec.label || null, dmRuns: runs?.length ?? 0, whole: runs?.filter((r) => r.ok).length ?? 0, rows: [] };
    report.scenarios.push(out);
    if (!runs?.length) { out.note = 'not run this time'; report.worse.push(`${sc}: the last run had it, this one did not run it`); continue; }
    if (out.whole < runs.length) report.worse.push(`${sc}: ${runs.length - out.whole} of ${runs.length} EDM runs did not end with every file whole`);
    if (!before?.length) { out.note = 'new: the last run did not have it'; report.notCompared.push(`${sc}: not in the last run`); continue; }
    let factor = 1;
    if (spec.scaleByCeiling) {
      const here = now.ceiling?.[spec.scaleByCeiling]?.best, then = last.ceiling?.[spec.scaleByCeiling]?.best;
      factor = here != null && then ? here / then : null;
      out.ceiling = { here: here ?? null, then: then ?? null, factor };
      if (factor === null) report.notCompared.push(`${sc}: times not compared, a run has no ceiling`);
    }
    const judged = spec.judged || null; // idle: memory and CPU only
    for (const m of B.metrics) {
      if (m.info || (judged && !judged.includes(m.key))) continue;
      const cur = median(runs.filter((r) => r.ok).map((r) => r[m.key]));
      const old = median(before.filter((r) => r.ok).map((r) => r[m.key]));
      if (cur == null && old == null) continue;
      const row = { metric: m.key, unit: m.unit, now: cur, last: old };
      const scaled = m.time && spec.scaleByCeiling;
      if (cur == null || old == null) row.verdict = cur == null ? 'not sampled now' : 'not sampled then';
      else if (scaled && factor === null) row.verdict = 'not compared: no ceiling';
      else {
        row.lastHere = +(old * (scaled ? factor : 1)).toFixed(3);
        row.allowed = +(row.lastHere * (1 + tolerance / 100) + (SLACK[m.unit] || 0)).toFixed(3);
        row.by = row.lastHere ? +(100 * (cur - row.lastHere) / row.lastHere).toFixed(1) : null;
        row.verdict = cur > row.allowed ? 'WORSE' : cur < row.lastHere ? 'better' : 'within tolerance';
        if (row.verdict === 'WORSE') report.worse.push(`${sc} ${m.key}: ${+cur.toFixed(3)} ${m.unit} against ${row.lastHere} ${m.unit} last time${scaled && factor !== 1 ? ` (${+old.toFixed(3)} x ceiling ${factor.toFixed(3)})` : ''}, over the ${row.allowed} allowed`);
      }
      out.rows.push(row);
    }
  }
  return report;
}

const n = (v) => (v == null ? '-' : Math.abs(v) >= 100 ? v.toFixed(0) : Math.abs(v) >= 10 ? v.toFixed(1) : v.toFixed(3).replace(/\.?0+$/, '') || '0');

export function markdown(report, now, last) {
  const where = (m) => (m?.system ? `${m.system.cpu}, ${m.system.logicalCores} threads, ${m.system.memoryGB} GB` : 'unknown') + (m?.build?.commit ? `, ${m.build.commit.head.slice(0, 10)}` : '');
  const o = ['## EDM against its own last numbers', '',
    `Worse means over the last run by more than ${report.tolerance}% plus the harness's slack (${Object.entries(report.slack).map(([u, v]) => `${v} ${u}`).join(', ')}); the machine-bound scenarios' times are scaled by the two runs' ceilings. Reference A's frozen floor is judged by \`compare.mjs\`, and the side-by-side run beside reference A stays the maintainer's.`, '',
    `This run: ${where(now?.meta)}. Last run: ${where(last?.meta)}.`, ''];
  for (const s of report.scenarios) {
    o.push(`### ${s.scenario}${s.label ? `: ${s.label}` : ''} (EDM runs ${s.dmRuns}, whole ${s.whole})`, '');
    if (s.ceiling) o.push(`Ceiling now ${n(s.ceiling.here)} s, then ${n(s.ceiling.then)} s: last times x ${s.ceiling.factor == null ? '-' : s.ceiling.factor.toFixed(3)}`, '');
    if (s.note) { o.push(s.note, ''); continue; }
    o.push('| Parameter | Last (here) | Allowed | Now | Change | Verdict |', '|---|---:|---:|---:|---:|---|');
    for (const r of s.rows) o.push(`| ${r.metric}${r.unit ? ` (${r.unit})` : ''} | ${n(r.lastHere ?? r.last)} | ${n(r.allowed)} | ${n(r.now)} | ${r.by == null ? '-' : `${r.by > 0 ? '+' : ''}${r.by}%`} | ${r.verdict === 'WORSE' ? '**WORSE**' : r.verdict} |`);
    o.push('');
  }
  o.push(report.worse.length ? `**Worse than the last run (${report.worse.length}):**\n\n- ${report.worse.join('\n- ')}` : 'No parameter is worse than the last run beyond the tolerance.');
  if (report.notCompared.length) o.push('', `Not compared: ${report.notCompared.join('; ')}`);
  return o.join('\n') + '\n';
}

// ─── Its own workings, on made-up runs.
function selfTest() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'edm-against-last-'));
  const write = (sub, name, body) => { fs.mkdirSync(path.join(dir, sub), { recursive: true }); fs.writeFileSync(path.join(dir, sub, `${name}.json`), JSON.stringify(body)); };
  const run = (o) => ({ app: 'edm', ok: true, seconds: 10, firstRequestMs: 500, lastByteToDoneMs: 200, refetchedPct: 1, peakWorkingSetMB: 50, peakPrivateMB: 30, cpuMs: 1000, ...o });
  try {
    write('last', 'ceiling', { scenario: 'ceiling', results: { fast: { best: 2 } } });
    write('now', 'ceiling', { scenario: 'ceiling', results: { fast: { best: 3 } } });
    // fast: a runner half as fast again; 14 s against 10 s x 1.5 is better, not worse.
    write('last', 'fast', { scenario: 'fast', runs: [run({ seconds: 10 }), run({ seconds: 10 })] });
    write('now', 'fast', { scenario: 'fast', runs: [run({ seconds: 14 }), run({ seconds: 14 })] });
    // per_conn: memory 50 -> 70 MiB is worse (allowed 50 x 1.2 + 3 = 63); CPU 1000 -> 1300 is within (1350).
    write('last', 'per_conn', { scenario: 'per_conn', runs: [run({})] });
    write('now', 'per_conn', { scenario: 'per_conn', runs: [run({ peakWorkingSetMB: 70, cpuMs: 1300 })] });
    // flaky: a file not whole; outage: in the last run, missing now; idle: memory and CPU only.
    write('last', 'flaky', { scenario: 'flaky', runs: [run({})] });
    write('now', 'flaky', { scenario: 'flaky', runs: [run({}), run({ ok: false })] });
    write('last', 'outage', { scenario: 'outage', runs: [run({})] });
    write('last', 'idle', { scenario: 'idle', runs: [{ app: 'edm', ok: true, peakWorkingSetMB: 40, peakPrivateMB: 20, cpuMs: 200 }] });
    write('now', 'idle', { scenario: 'idle', runs: [{ app: 'edm', ok: true, peakWorkingSetMB: 41, peakPrivateMB: 20, cpuMs: 250 }] });
    const now = load([path.join(dir, 'now')]), last = load([path.join(dir, 'last')]);
    const r = judge(now, last, 20);
    const row = (sc, k) => r.scenarios.find((s) => s.scenario === sc)?.rows.find((x) => x.metric === k);
    const expect = (ok, what) => { if (!ok) throw new Error(`self-test: ${what}\n${JSON.stringify(r, null, 1)}`); };
    expect(row('fast', 'seconds').lastHere === 15 && row('fast', 'seconds').verdict === 'better', 'fast seconds scaled by the ceiling (10 x 1.5)');
    expect(row('fast', 'peakWorkingSetMB').lastHere === 50, 'memory is not scaled by the ceiling');
    expect(row('per_conn', 'peakWorkingSetMB').verdict === 'WORSE', 'memory 50 -> 70 MiB is worse');
    expect(row('per_conn', 'cpuMs').verdict === 'within tolerance', 'CPU 1000 -> 1300 ms is within 20% + 150 ms');
    expect(row('per_conn', 'seconds').verdict === 'within tolerance' && row('per_conn', 'seconds').by === 0, 'the same seconds are within');
    expect(row('idle', 'cpuMs').verdict === 'within tolerance' && !row('idle', 'seconds'), 'idle judges memory and CPU only');
    expect(r.worse.some((w) => w.startsWith('flaky:')) && r.worse.some((w) => w.startsWith('outage:')), 'a file not whole and a scenario missing are worse');
    expect(r.worse.length === 3, 'three things worse');
    const md = markdown(r, now, last);
    expect(md.includes('**WORSE**') && md.includes('Worse than the last run (3)'), 'the summary names what is worse');
    expect(judge(now, now, 20).worse.length === 1, 'a run against itself is worse only where a file was not whole');
    console.log('against-last.mjs self-test: ok');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const flag = (name) => { const i = args.indexOf(name); if (i < 0) return null; const v = args[i + 1]; args.splice(i, 2); return v; };
  const has = (name) => { const i = args.indexOf(name); if (i < 0) return false; args.splice(i, 1); return true; };
  if (has('--self-test')) { selfTest(); process.exit(0); }
  const asJson = has('--json');
  const tolerance = Number(flag('--tolerance') ?? process.env.BENCH_TOLERANCE ?? 20);
  const mdFile = flag('--markdown');
  const die = (why) => { console.error(why); process.exit(2); };
  if (!(tolerance >= 0)) die('--tolerance is a percentage, 0 or more');
  if (args.length !== 2) die('usage: node bench/against-last.mjs <this run\'s results> <last run\'s results> [--tolerance <percent>] [--markdown <file>] [--json]');
  for (const p of args) if (!fs.existsSync(p)) die(`no results at ${p}`);
  const now = load([args[0]]), last = load([args[1]]);
  if (!now.scenarios.size) die(`No scenario results with EDM in ${args[0]}.`);
  if (!last.scenarios.size) die(`No scenario results with EDM in ${args[1]}.`);
  if ((now.meta?.dmConnections ?? null) !== (last.meta?.dmConnections ?? null)) die(`The runs used different connections settings (${now.meta?.dmConnections ?? 'EDM\'s own'} and ${last.meta?.dmConnections ?? 'EDM\'s own'}): not comparable.`);
  const report = judge(now, last, tolerance);
  const md = markdown(report, now, last);
  if (mdFile) fs.writeFileSync(mdFile, md, 'utf8');
  console.log(asJson ? JSON.stringify(report, null, 1) : md);
  process.exit(report.worse.length ? 1 : 0);
}
