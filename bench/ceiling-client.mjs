// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// The reference client of `bench.mjs ceiling`, in a process of its own so it does not share the
// server's event loop: N ranges of URL fetched at once, each written to a file of its own in DIR
// through a 4 MiB buffered stream. Prints the seconds from the first request to the last file
// closed, or exits 1. It is what the machine and its disk allow the simplest segmented client; it
// is not dm and not reference A.
//
//   node bench/ceiling-client.mjs URL SIZE N DIR
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

const [url, sizeArg, nArg, dir] = process.argv.slice(2);
const size = Number(sizeArg), n = Number(nArg);
const part = Math.floor(size / n);
const t0 = process.hrtime.bigint();
const one = (i) => new Promise((resolve, reject) => {
  const a = i * part, b = i === n - 1 ? size - 1 : (i + 1) * part - 1;
  http.get(url, { headers: { range: `bytes=${a}-${b}` }, agent: false }, (res) => {
    if (res.statusCode !== 206) return reject(new Error(`range ${i}: HTTP ${res.statusCode}`));
    const out = fs.createWriteStream(path.join(dir, `p${i}`), { highWaterMark: 4 * 1024 * 1024 });
    let got = 0;
    res.on('data', (d) => { got += d.length; });
    res.on('error', reject);
    res.pipe(out);
    out.on('close', () => (got === b - a + 1 ? resolve() : reject(new Error(`range ${i}: ${got} of ${b - a + 1} bytes`))));
    out.on('error', reject);
  }).on('error', reject);
});
try {
  await Promise.all([...Array(n).keys()].map(one));
  console.log((Number(process.hrtime.bigint() - t0) / 1e9).toFixed(3));
} catch (e) {
  console.error(e.message);
  process.exit(1);
}
