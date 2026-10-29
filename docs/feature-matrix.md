# Feature matrix: dm's engine against IDM and FDM

What the reference does is in [reference-behavior.md](reference-behavior.md).
"Match" is claimed only where a named test checks it.

| Behaviour | Reference | dm | Evidence |
|---|---|---|---|
| Split while downloading; a free connection halves the largest range left | IDM, Verified | Match | `download/ranged.rs` `split_largest`; `tests/speed.rs` |
| Finished connections reused, no new connect | IDM, Verified | Match | `tests/reuse.rs`: 11 requests on 4 TCP connections for 32 MB |
| Positions saved often; resume after a kill | IDM, Verified ("several times per minute") | Match, more often (every second or 4 MB) | `tests/download.rs`; QA `resume_after_kill9` |
| Connections per download, default and cap | IDM, Verified | dm: up to 32, by setting | `plan.rs` `MAX_CONNECTIONS` |
| Fewer connections for some servers | IDM Exceptions, Verified | Match: site limits, and a server that refuses a connection is given fewer | `Options::host_limits`; `ranged.rs` `refused`; `settings/tests.rs` |
| Error recovery and retries | IDM, Verified | Match | `retry.rs`; QA `stalls_and_cuts`, `network_loss_60s` |
| Queues, scheduler, shut down when done | IDM, Verified | Match | `parity.md` section on the queue |
| Re-download a file that changed | IDM synchronisation, Verified | Match | `queue/recheck.rs` |
| Quota per hour | IDM, Verified | Match (a download allowance) | `quota.rs`; `queue.rs` `a_recheck_time_in_the_future_is_stale_not_waited_out` |
| Categories | IDM, Verified | Match | `category.rs` |
| Basic, NTLM, Negotiate | IDM, Verified | Partial: Basic, NTLM, and Negotiate carrying NTLM; no Kerberos | `tests/auth.rs`, `tests/ntlm.rs` |
| Scanner on completion | IDM, Verified | Match: a program run on each finished file (a virus checker or a script) | `dm-shell/src/settings.rs` |
| Grabber and "Download all" | IDM, Verified | Match | `dm-desktop` site files; extension "Download all links" |
| A page's only video named by the page | IDM and FDM, Reported | Match since 2026-10-29 | `ytdlp/fetch.rs` `a_pages_only_video_is_named_without_the_number_ytdlp_gives_it`; QA `video_page` |
