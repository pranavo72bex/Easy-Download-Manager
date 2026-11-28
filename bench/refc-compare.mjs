// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// EDM's runs of reference C's seven scenarios (`node bench/bench.mjs refc`) beside the medians
// reference C published for its own program (bench/reference-c-published.json, gaps row C0):
//
//   node bench/refc-compare.mjs <results folder>
//
// Each parameter EDM is behind on is marked. Its numbers are from another computer: a parameter
// bound by the machine (fast1g's time, CPU) compares loosely; one bound by the scenario's own
// caps (capped, latency, slowconn, maxconns, drops) or by a fixed cost (small256k, the phases)
// compares closely.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ref = JSON.parse(fs.readFileSync(path.join(HERE, 'reference-c-published.json'), 'utf8')).scenarios;
const dir = process.argv[2];
if (!dir) { console.error('usage: node bench/refc-compare.mjs <results folder>'); process.exit(2); }
const median = (xs) => { const v = xs.filter((x) => typeof x === 'number').sort((a, b) => a - b); return v.length ? v[Math.floor((v.length - 1) / 2)] : null; };
const rows = [];
let behind = 0;
for (const [name, c] of Object.entries(ref)) {
  const file = fs.readdirSync(dir).find((f) => f.startsWith(`${name}`) && f.endsWith('.json'));
  if (!file) { rows.push(`${name.padEnd(10)} not run`); continue; }
  const runs = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf8')).runs.filter((r) => (r.app === 'edm' || r.app === 'dm') && r.ok);
  const m = (k) => median(runs.map((r) => r[k]));
  const pairs = [
    ['time', m('seconds') === null ? null : m('seconds') * 1000, c.medianMs],
    ['start', m('firstRequestMs'), c.startupMs],
    ['finish', m('lastByteToDoneMs'), c.finishMs],
    ['CPU', m('cpuMs'), c.cpuMs],
    ['memory', m('peakWorkingSetMB'), c.peakMemoryMB],
  ];
  const cells = pairs.map(([k, ours, theirs]) => {
    if (ours === null) return `${k} -`;
    const lost = ours > theirs * 1.05;
    if (lost) behind++;
    return `${k} ${Math.round(ours)}/${Math.round(theirs)}${lost ? ' BEHIND' : ''}`;
  });
  rows.push(`${name.padEnd(10)} ${runs.length} runs  ${cells.join('  ')}`);
}
console.log('EDM / reference C (ms, MB): medians\n' + rows.join('\n'));
console.log(`\n${behind} parameter(s) where EDM is behind by more than 5%`);
