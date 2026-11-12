// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// Judges dm's benchmark results against reference A's frozen baseline, parameter by parameter.
//
//   node bench/compare.mjs [results folder or result files ...] [--group <name>] [--json]
//
// With no folder, the newest under bench/results. The group is `standard` unless the results were
// run with BENCH_DM_CONNECTIONS=16 (then `connections16`) or --group names another.
// Exit status: 0 dm is better than the floor in every parameter of every scenario it ran;
// 1 worse somewhere, a file not whole, or a machine-bound scenario run without `ceiling`;
// 2 the baseline was changed, or there is nothing to judge.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BASELINE = path.join(HERE, 'reference-a-baseline.json');
// The baseline's SHA-256 with \n line endings. The xtask test
// `the_reference_a_baseline_never_changes` pins the same digest.
const PINNED = '113366e4f1b8c4c1fca96c4285fcbfd706f6d72a7b8cc67fcb88b75447f568f8';

const args = process.argv.slice(2);
const flag = (name) => { const i = args.indexOf(name); if (i < 0) return null; const v = args[i + 1]; args.splice(i, 2); return v; };
const asJson = args.includes('--json'); if (asJson) args.splice(args.indexOf('--json'), 1);
const groupAsked = flag('--group');
const die = (code, why) => { console.error(why); process.exit(code); };

const text = fs.readFileSync(BASELINE, 'utf8').replace(/\r\n/g, '\n');
const digest = crypto.createHash('sha256').update(text).digest('hex');
if (digest !== PINNED) die(2, `bench/reference-a-baseline.json was changed (SHA-256 ${digest}, frozen as ${PINNED}). It never changes: restore it with git, and put a new measurement in a file of its own.`);
const B = JSON.parse(text);
const median = (xs) => { const v = xs.filter((x) => x != null).sort((a, b) => a - b); if (!v.length) return null; const m = v.length >> 1; return v.length % 2 ? v[m] : (v[m - 1] + v[m]) / 2; };
const judged = B.metrics.filter((m) => !m.info);

// Every floor is what the archived runs say it is.
for (const [group, scenarios] of Object.entries(B.floors)) {
  for (const [sc, e] of Object.entries(scenarios)) {
    if (!e.floor) continue;
    const ok = B.runs.filter((r) => r.group === group && r.scenario === sc && r.ok);
    for (const m of judged) {
      const vals = ok.map((r) => r[m.key]).filter((v) => v != null);
      if (vals.length && e.floor[m.key]?.best !== Math.min(...vals)) die(2, `baseline floor ${group}/${sc}/${m.key} is not the best of its runs`);
    }
  }
}

// ─── The results.
let inputs = args;
if (!inputs.length) {
  const root = path.join(HERE, 'results');
  const dirs = fs.existsSync(root) ? fs.readdirSync(root).map((d) => path.join(root, d)).filter((d) => fs.statSync(d).isDirectory()) : [];
  dirs.sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs);
  if (!dirs.length) die(2, 'No results: run `node bench/bench.mjs` first, or name a results folder.');
  inputs = [dirs[0]];
}
const files = inputs.flatMap((p) => (fs.statSync(p).isDirectory() ? fs.readdirSync(p).filter((f) => f.endsWith('.json')).map((f) => path.join(p, f)) : [p]));
const results = files.map((f) => { try { return { f, j: JSON.parse(fs.readFileSync(f, 'utf8')) }; } catch { return null; } }).filter(Boolean);
const ceilingHere = results.find((r) => r.j.scenario === 'ceiling')?.j;
const byScenario = new Map();
for (const { f, j } of results) {
  if (!Array.isArray(j.runs) || !j.scenario || j.scenario === 'inet') continue;
  const e = byScenario.get(j.scenario) || { files: [], dm: [], refa: [], meta: j.meta };
  e.files.push(path.basename(f));
  for (const r of j.runs) {
    const run = { ...r, transferSeconds: r.seconds != null && r.firstRequestMs != null ? r.seconds - r.firstRequestMs / 1000 : null };
    if (r.app === 'dm') e.dm.push(run); else if (r.app === 'refa') e.refa.push(run);
  }
  byScenario.set(j.scenario, e);
}
if (!byScenario.size) die(2, `No scenario results with dm in ${inputs.join(', ')}.`);
const anyMeta = [...byScenario.values()][0].meta || {};
const group = groupAsked || (anyMeta.dmConnections === 16 ? 'connections16' : 'standard');
if (!B.floors[group]) die(2, `No group ${group} in the baseline (${Object.keys(B.floors).join(', ')}).`);

// ─── Judging.
const report = { baseline: { digest, frozen: B.frozen }, group, system: anyMeta.system || null, dmBinary: anyMeta.dmBinary || null, scenarios: [], worse: [], notJudged: [] };
const order = ['idle', 'fast', 'per_conn', 'flaky', 'outage', 'batch', 'big40'];
for (const sc of [...byScenario.keys()].sort((a, b) => order.indexOf(a) - order.indexOf(b))) {
  const got = byScenario.get(sc);
  const entry = B.floors[group][sc];
  const spec = B.scenarios[sc];
  const out = { scenario: sc, label: spec?.label, dmRuns: got.dm.length, whole: got.dm.filter((r) => r.ok).length, rows: [] };
  report.scenarios.push(out);
  if (!got.dm.length) { out.note = 'no dm runs'; continue; }
  if (out.whole < got.dm.length) report.worse.push(`${sc}: ${got.dm.length - out.whole} of ${got.dm.length} dm runs did not end with every file whole`);
  if (!entry?.floor) { out.note = `no reference A floor in ${group}: ${B.notMeasured?.[sc] || entry?.why || 'not measured'}`; report.notJudged.push(`${sc}: no floor`); }
  let factor = 1;
  if (spec?.scaleByCeiling) {
    const here = ceilingHere?.results?.[spec.scaleByCeiling]?.best;
    if (here == null) { factor = null; report.worse.push(`${sc}: machine-bound, and these results have no ceiling (run \`node bench/bench.mjs ceiling\` on this computer)`); }
    else factor = Math.min(1, here / B.ceiling.results[spec.scaleByCeiling].best);
    out.ceiling = { here, baseline: B.ceiling.results[spec.scaleByCeiling].best, factor };
  }
  const ok = got.dm.filter((r) => r.ok);
  for (const m of B.metrics) {
    const vals = ok.map((r) => r[m.key]).filter((v) => v != null);
    const refaHere = median(got.refa.filter((r) => r.ok).map((r) => r[m.key]));
    const f = entry?.floor?.[m.key];
    if (!vals.length && !f) continue;
    if (entry?.floor && !f && !m.info) continue; // a parameter this scenario does not judge (idle: memory and CPU only)
    const row = { metric: m.key, unit: m.unit, dmMedian: median(vals), dmBest: vals.length ? Math.min(...vals) : null, dmWorst: vals.length ? Math.max(...vals) : null, refaThisRun: refaHere };
    if (m.info) { row.refaMedian = f?.median ?? null; row.verdict = 'recorded'; }
    else if (!f) row.verdict = 'no floor';
    else if (!vals.length) row.verdict = 'not sampled here';
    else if (m.time && factor === null) { row.floor = f.best; row.verdict = 'not judged: no ceiling'; }
    else {
      row.floor = +(f.best * (m.time ? factor : 1)).toFixed(3);
      row.floorAsFrozen = f.best;
      const better = row.dmMedian < row.floor || (row.floor === 0 && row.dmMedian === 0);
      row.verdict = better ? 'better' : 'WORSE';
      row.by = row.floor ? +(100 * (row.dmMedian - row.floor) / row.floor).toFixed(1) : null;
      if (!better) report.worse.push(`${sc} ${m.key}: dm ${+row.dmMedian.toFixed(3)} ${m.unit} against reference A's floor ${row.floor} ${m.unit}`);
    }
    out.rows.push(row);
  }
}

if (asJson) console.log(JSON.stringify(report, null, 1));
else {
  const n = (v) => (v == null ? '-' : Math.abs(v) >= 100 ? v.toFixed(0) : Math.abs(v) >= 10 ? v.toFixed(1) : v.toFixed(3).replace(/\.?0+$/, '') || '0');
  console.log(`Baseline frozen ${B.frozen} (SHA-256 ${digest.slice(0, 12)}…), group ${group}`);
  if (report.system) console.log(`Measured on: ${report.system.cpu}, ${report.system.logicalCores} threads, ${report.system.memoryGB} GB, ${report.system.platform} ${report.system.release}`);
  for (const s of report.scenarios) {
    console.log(`\n## ${s.scenario}: ${s.label || ''}  (dm runs ${s.dmRuns}, whole ${s.whole})`);
    if (s.ceiling) console.log(`   ceiling here ${n(s.ceiling.here)} s, baseline ${n(s.ceiling.baseline)} s: time floors x ${s.ceiling.factor == null ? '-' : s.ceiling.factor.toFixed(3)}`);
    if (s.note) console.log(`   ${s.note}`);
    console.log(`   ${'metric'.padEnd(24)}${'reference A floor'.padStart(19)}${'dm median'.padStart(12)}${'dm best..worst'.padStart(20)}  verdict`);
    for (const r of s.rows) {
      const floor = r.floor != null ? `${n(r.floor)}${r.floorAsFrozen != null && r.floorAsFrozen !== r.floor ? ` (${n(r.floorAsFrozen)})` : ''}` : r.refaMedian != null ? `median ${n(r.refaMedian)}` : '-';
      const verdict = r.verdict === 'better' || r.verdict === 'WORSE' ? `${r.verdict}${r.by != null ? ` (${r.by > 0 ? '+' : ''}${r.by}%)` : ''}` : r.verdict;
      console.log(`   ${(r.metric + (r.unit ? ` ${r.unit}` : '')).padEnd(24)}${floor.padStart(19)}${n(r.dmMedian).padStart(12)}${`${n(r.dmBest)}..${n(r.dmWorst)}`.padStart(20)}  ${verdict}${r.refaThisRun != null ? `   [reference A this run ${n(r.refaThisRun)}]` : ''}`);
    }
  }
  console.log(report.worse.length ? `\nWORSE than reference A's floor (${report.worse.length}):\n- ${report.worse.join('\n- ')}` : '\ndm is better than reference A\'s floor in every judged parameter.');
  if (report.notJudged.length) console.log(`Not judged: ${report.notJudged.join('; ')}`);
}
process.exit(report.worse.length ? 1 : 0);
