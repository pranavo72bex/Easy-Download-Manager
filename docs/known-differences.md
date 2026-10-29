# Known differences from IDM and FDM

Differences found and left, and why. Anything here that changes is moved to
[feature-matrix.md](feature-matrix.md) with its test.

- **Smallest piece split.** IDM stops splitting at a size "too small for this
  connection type" and gives no number. dm uses a fixed 1 MiB (`MIN_SEGMENT`
  for the plan, `MIN_SPLIT` for halving). Not compared: IDM's value is not
  known.
- **Default connections.** IDM is reported to default to 8 and advises at most
  4; dm's default comes from its settings. Left: changing the default changes
  a setting people see, which is a UI matter.
- **Videos on pages.** IDM reads a player's own requests and needs no helper;
  dm has yt-dlp read the page (`docs/decisions.md`, "Why not without
  yt-dlp"). Same files result where both work; YouTube needs yt-dlp.
- **A video yt-dlp downloads itself.** When dm cannot fetch a page's streams
  and yt-dlp downloads them instead, the file keeps yt-dlp's name, so a page's
  only `<video>` is still numbered "(1)" on that path. Only the path where dm
  fetches the streams, the usual one, drops the number.
- **Kerberos.** IDM signs in with Kerberos; dm's Negotiate carries NTLM only.
  Left: it needs a Kerberos realm to test against and a system library.
- **Not compared by running.** Neither reference program could be run here
  (IDM is Windows-only; FDM was not installed). Every "match" is against
  their documented behaviour, not a side-by-side run.
