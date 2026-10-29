# Known differences from two other download managers

Differences found and left, and why. Anything here that changes is moved to
[feature-matrix.md](feature-matrix.md) with its test.

- **Smallest piece split.** Reference A stops splitting at a size "too small for this
  connection type" and gives no number. dm uses a fixed 1 MiB (`MIN_SEGMENT`
  for the plan, `MIN_SPLIT` for halving). Not compared: reference A's value is not
  known.
- **Default connections.** Reference A is reported to default to 8 and advises at most
  4; dm's default comes from its settings. Left: changing the default changes
  a setting people see, which is a UI matter.
- **Videos on pages.** Reference A reads a player's own requests and needs no helper;
  dm has yt-dlp read the page (`docs/decisions.md`, "Why not without
  yt-dlp"). Same files result where both work; YouTube needs yt-dlp.
- **A video yt-dlp downloads itself.** When dm cannot fetch a page's streams
  and yt-dlp downloads them instead, the file keeps yt-dlp's name, so a page's
  only `<video>` is still numbered "(1)" on that path. Only the path where dm
  fetches the streams, the usual one, drops the number.
- **YouTube without a helper.** Reference A reads YouTube's player and
  ships a build whenever YouTube changes (a paid team's work). dm's reading
  of the player's requests, which it does on every site, finds nothing on
  YouTube today: its web player streams by SABR, a POST whose body names
  the format and whose answer is framed (measured 2026-10-29, 13 of 13 media
  requests). A YouTube client of dm's own would need its protocol, its
  proof-of-origin tokens and keeping up with every change, which is the
  upkeep the maintainer wants none of; yt-dlp, kept up by many and fetched
  by dm afresh each week, is kept instead (`decisions.md`, 2026-10-29). The
  one helper-free way left, saving what the player plays as it plays, gives
  only the quality the player picked at the speed it plays; weighed and not
  taken.
- **Kerberos.** Reference A signs in with Kerberos; dm's Negotiate carries NTLM only.
  Left: it needs a Kerberos realm to test against and a system library.
- **Connections on a slow server.** Reference B keeps opening connections
  while each is slow (29 for one file held to 2 MiB/s a connection, 1.2 s);
  dm keeps to the number set (8, 2.0 s). Left for the maintainer: more than
  the person set is a change of behaviour, and some servers block clients
  that open many (feature-matrix.md, "Worth taking").
- **Open-ended ranges.** Reference B asks `bytes=N-` and hangs up at the next
  piece; dm asks closed ranges. Left: a closed range never reads past its
  end, and dm's tail check on resume relies on it.
- **Reference A not run.** It is Windows-only and no Windows machine was
  reachable; every comparison with it is against its own documentation.
  Reference B was run in Docker (Linux build), 2026-10-29.
