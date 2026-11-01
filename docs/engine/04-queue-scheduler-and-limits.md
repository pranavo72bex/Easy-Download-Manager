# 04. Queue, scheduler and limits

Goal: many downloads are held, ordered, started and stopped by rules the
person can read, and nothing is lost when dm stops.

## States of a row

| State | Meaning | Leaves by |
|---|---|---|
| Queued | Accepted, waiting for a slot or for the schedule | start, pause, cancel |
| Starting | Probing | probe result |
| Running | Connections open | pause, error, complete |
| Paused / Stopped | Person or rule stopped it; bytes kept | resume |
| Waiting for network | All connections failing with network errors; bytes kept | network returns, pause |
| Waiting for link | Signed address expired; waits for the browser's next hand-over (S21) | hand-over, pause |
| Failed | A non-retryable error, or tries used up; bytes kept | retry, remove |
| Complete | File in place, checks passed | remove, open, re-download |
| Cancelled | Person discarded it | remove |

Every transition is one function in `queue/` and one test. The list the
window shows is read from the queue's own rows, not a copy (decisions,
2026-10-31, 2026-10-28).

## Requirements

| ID | Requirement | Status | Code | Test |
|---|---|---|---|---|
| Q1 | At most N downloads run at once (default 3). The rest wait in order and start as slots free (B18). | BUILT | `queue/mod.rs`, `queue/runner.rs`, `tests/queue.rs` | Add five; sample the running count every 40 ms; never more than 3. |
| Q2 | Several named queues (a main queue and others), each with its own concurrency, schedule and folder rules; "Add for later" puts a row in a queue that does not start until told (B19). Start queue and Stop queue act on one named queue. | BUILT (main queue); PARTIAL (named queues, see gaps row 24) | `queue/queues.rs`, `queue/actions.rs` | Two queues; start one; the other's rows do not move. |
| Q3 | Order: a row can move up and down; a queue runs in order; a row started by hand outside the schedule runs once. | BUILT | `queue/edit.rs`, `actions.rs` | Reorder, start; log shows order. |
| Q4 | Pause, resume, cancel, stop all, start queue, retry, remove, clear finished, undo remove; all from the window, the command line and the control port with the same effect. A pause followed at once by a resume is honoured. | BUILT | `queue/actions.rs`, `control.rs`, gaps state audit 2026-10-28 | Random sequence of these over 8 downloads for 60 s, compare queue state to the window's read every 40 ms (the audit's harness). |
| Q5 | Persistence: the queue file is written at most every 150 ms; a long list saves only what changed to a journal; the file is written atomically; secrets are in a separate file with owner-only permission, sealed by the system's store where there is one. | BUILT (Windows sealing is written, not yet built, gaps row 39) | `queue/persist.rs`, `queue/journal.rs`, `secretstore.rs` | Kill dm during a write at random points; restart; every row returns in its state (B21, B22). Flip a byte in the queue file: dm starts with the last good copy and says so. |
| Q6 | Crash recovery: a row that was Running comes back Paused or auto-resumes per the setting, from its ledgers. | BUILT | `queue/persist.rs`, `ranged/staged.rs` | B21. |
| Q7 | Schedule: a window in local time (from, to, days), wrapping past midnight; rows start when it opens and stop when it closes; an option to start rows held by it by hand ("Start now"). | BUILT | `schedule.rs` | Table tests over midnight, DST change days (23 and 25 hour days), empty days list meaning every day. |
| Q8 | After the scheduled downloads: do nothing, exit dm, sleep, hibernate, shut down; hang up a dial-up link is not offered (B49). The action runs only when the queue is empty and nothing failed that will be retried; the person gets a short warning they can cancel. | BUILT (Windows/macOS/Linux commands differ; check each) | `dm-desktop` platform files | Run each action in a VM; the warning appears; Cancel stops it. |
| Q9 | Tries again for failed downloads: 0 to 5 rounds at the end of the queue (B49). | BUILT | `queue/runner.rs`, `retry.rs` | Server failing the first two attempts of one file. |
| Q10 | Speed limit: one ceiling shared by all connections of all downloads, set by presets (Low 256 KB/s, others) or exactly; a token bucket in slices of 250 ms; a limit lifts at once when switched off (B20, B52). The shown byte count follows the limit. | BUILT | `limit.rs`, `tests/speed.rs`, `tests/limits.rs` | Five downloads together stay within 5% of the ceiling over 30 s; a limit changed mid-run takes effect within a second. |
| Q11 | Per-download limit in addition to the global one. | TO BUILD | none | Two rows, one limited to 100 KB/s: the log shows its rate; the other unaffected. |
| Q12 | Download quota ("limits"): no more than so many megabytes every so many hours; when reached, running rows are stopped, held "quota reached" and resume when the period rolls; the period's counter survives a restart. | BUILT | `quota.rs` | Quota 10 MB per period with a short test period; a 30 MB file stops at 10 MB and goes on in the next period. |
| Q13 | Shaped by time of day: a speed ceiling differing by hour (for example full speed at night). | PARTIAL: schedule and limit exist; a per-hour limit table is not confirmed | `schedule.rs`, `limit.rs` | Table of hours with limits; the rate changes at the boundary. |
| Q14 | Duplicate address: ask first with Download again, Show it, Replace the old file, Cancel (B17, B50). | BUILT | `queue/mod.rs`, window | B17, B50. |
| Q15 | Two rows never write the same file; names are claimed atomically. | BUILT | `download/files.rs` (`claim_path`) | Add the same name twice at once. |
| Q16 | Categories: a finished file goes to a folder by kind (by extension and type; user rules; by site). | BUILT | `category.rs` | B16. |
| Q17 | Delete the file after a delay (10 minutes, 1 hour, 1 day) and watched folder and site rules. | BUILT | `queue/mod.rs` (`delete_due_in`), `docs/decisions.md` 2026-10-24 | Short delay in a test. |
| Q18 | Periodic synchronisation: download again when the server's file changed, on a timer. | PARTIAL (recheck exists; a timer is not confirmed) | `queue/recheck.rs` | Change the file; the timer triggers a probe and a new row. |
| Q19 | Site grabber: follow links from a start page to a depth and add files matching filters. | PARTIAL: a site search that stays on its site and stops at 100 pages exists | `docs/decisions.md` 2026-10-16 | Local site with 3 levels of pages; filters pick `.zip` only. |
| Q20 | Disk space check and the shared promise between rows (S8). | BUILT | `download/space.rs` | S8. |
| Q21 | Notifications on finish and failure; held while a full-screen app is on. | PARTIAL (gaps row 14: unrun on Windows) | `dm-desktop` | Gaps row 14. |
| Q22 | Clipboard watch: offer an address copied from a browser. | PARTIAL (gaps row 20: untested) | `dm-desktop` | Gaps row 20. |

## Notes

* The queue is the only writer of row state. The window reads; the runner
  reports through the queue.
* A scheduler action that ends the computer must not fire while a row is
  still Running or a retry round is pending.
* Time zones: store absolute times (UTC); show local.
