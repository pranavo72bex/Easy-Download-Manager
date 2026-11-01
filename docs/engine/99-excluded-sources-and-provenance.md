# 99. Excluded sources and provenance

## Rule applied

The blueprint is clean room. It uses only: public standards, dm's own code
and docs, reference A's public pages, and black-box observation of
reference A run normally against a local server (already recorded in
`docs/reference-behavior.md`). Anything that looks derived from reading a
program's code, disassembly, decompiler output, binary strings, imported
function names, class or method names found in a binary, or its artwork was
read only to classify it and was not used. Plans to reverse-engineer a
program are not followed or summarised.

## Excluded sources

The maintainer's notes come in three sets. All three were opened to decide
whether they are usable. None of their content is used. File names only:

### First set (about the commercial manager installed on the maintainer's computer)

| File | Why excluded |
|---|---|
| `README.md` | Index of the set; its facts come from static reading of the program's binaries. |
| `architecture.md` | Process, library and driver inventory taken from the binaries' imports and resources. |
| `browser-extension.md` | A reading of the extension's shipped script and of private message formats. Reference A's private protocol is not needed: dm has its own (file 08). |
| `download-engine.md` | Engine description built from class and method names found in the binary. |
| `functions.md` | A list of internal class and method names recovered from the binary. |
| `rea-analysis.md` | Output of a binary-analysis tool run on the program. |
| the `ghidra` sub-folder (index, raw exports, findings, log) | Decompiler and analysis output. |

### Second set (a plan to analyse the program)

| File | Why excluded |
|---|---|
| `README.md` | Introduces a plan to reverse-engineer the program on another operating system. |
| `analysis-plan.md` | A command-by-command plan to decompile and inspect the binary. Not followed. |
| `expected-results.md` | Predicted results of that plan. |
| the overview file | A summary of facts from earlier binary analysis. |
| `what-is-rea.md` | Describes the analysis tool for that plan. |
| `linux-setup.md` | Setup steps for that plan. |

### Third set (results of that analysis and a comparison)

| File | Why excluded |
|---|---|
| `README.md` | Index of decompilation results. |
| `architecture.md` | Built from decompiler output and binary analysis. |
| `browser-integration.md` | Built from the same. |
| `download-engine.md` | Built from decompiler output (named routines, import tables). |
| `ghidra-decompilation.md` | Decompiler output and its interpretation. |
| `runtime-execution.md` | Process-level inspection of the running program (memory, modules); not black-box on the wire. |
| `cleanroom-parity.md` | Despite its title, it states what "decompilation revealed" and builds its comparison on that. Its recommendations for dm (user-mode catching, writing ranges in place) are already in dm's own design and are justified here by standards and by dm's own measurements instead. |

Nothing from these files was copied, paraphrased or used as a hint, including
numbers (connection defaults, limits), names of messages, address schemes,
registry or file-system layout, error message wording, or protocol details.
Where a similar fact appears in this blueprint it comes from the sources
below.

## Sources used

| Source | Kind | Used for |
|---|---|---|
| RFC 9110, 9111, 9112, 7233, 6265, 7616, 7617, 8187, 3986, 8446, 1928, 1929, 959, 3659, 4217, 8216 | Public standards | Files 02, 03, 05 |
| ISO/IEC 23009-1, 14496-3, 14496-10, 14496-12, 14496-14, 14496-15, 13818-1, 11172-3; Matroska; GIF89a; ZIP appnote; WebVTT; RTMP and RTSP specifications | Public standards | Files 05, 07 |
| `CLAUDE.md`, `docs/how-it-works.md`, `docs/architecture.md`, `docs/decisions.md`, `docs/gaps.md`, `docs/behaviour-contract.md` | dm's own documents | Status marks, rules, rows |
| `docs/reference-behavior.md` | Reference A's public pages; reference A run normally against a local server (Observed, 2026-10-31); reference B's public documentation and open add-on | Observations quoted by ID in files 03, 10 |
| `docs/clean-room-download-engine-spec.md` | A clean-room specification of a third manager, from run-time observation | Background on how a clean-room record is written (file layout of a download, states, error cases); not about reference A |
| `desktop/crates/dm-core/src` and `tests` | dm's own code | Every BUILT, PARTIAL and TO BUILD mark and code path |

## For anyone adding to this folder

* Add a fact only with a source from the table above or a new black-box
  measurement recorded in `docs/reference-behavior.md`.
* If a document you are given contains names of functions, strings from a
  binary, decompiled code, or a plan to take a program apart, do not use it.
  Add its file name and the reason to the tables above, and no content.
* Name the commercial manager "reference A" and the free one "reference B"
  in file names and text.
