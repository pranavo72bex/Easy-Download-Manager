# R&D: what will break, how often, how we'd notice

From yt-dlp's release history 2025-2026 ([extractor-research.md](extractor-research.md)).
The question for each risk: does a download *fail*, or only get slower
because yt-dlp takes over?

| What breaks | How often | Effect on dm | How we'd notice | Fix |
|---|---|---|---|---|
| YouTube changes the player JS so the `n`/sig solver fails | A few times a year, sometimes 4 in 10 days | Native read fails, and yt-dlp reads instead (with its newer solver, fetched within a week or at once on failure): **slower, not broken** | Native-only health check; log line "could not read the YouTube page itself" | Bump `SOLVER_VERSION` and digests to the new `yt-dlp-ejs` release (upstream watcher flags it) |
| The TV client is retired or its version is refused | Every 2-3 months a client changes | Same as above: yt-dlp takes over | Health check; watcher sees `_base.py` change | Port the new client name and version from `_base.py` |
| PO tokens enforced for the TV client too | About 3 enforcement steps in 18 months; not yet for TV | Native fails; yt-dlp also fails without a token provider. **Downloads of the affected videos fail**, with "sign in / not a bot" in dm's words | Full-chain health check; user reports | Gaps row 37 (a token provider); until then, the browser's own cookies help |
| YouTube goes SABR-only for every client | Not happened; `web` has been SABR-only since Apr 2025 | Both readers fail for YouTube | Health check, both stages | A SABR client (protobuf + UMP), large; or follow yt-dlp's PR #13515 once merged |
| "Confirm you're not a bot" from an IP address (VPN, VPS, CI) | Depends on the network, not on code | That person's YouTube downloads fail | Their report; CI false alarms | The browser's cookies (already passed); nothing in code |
| yt-dlp itself breaks for a site | Weekly somewhere across 940 sites | That site fails until yt-dlp's fix (usually days) | User reports; dm re-fetches yt-dlp at failure | None in dm: yt-dlp's release |
| Old QuickJS very slow on new solver | When the fetched build ages | Reads take minutes | Health check timing | Keep the fetched QuickJS current (`tools.rs`) |
| Solver scripts bundled later without notices | Only if we bundle | License breach (MIT/ISC notices) | Release review | Ship meriyah's and astring's notices with them |
| CI health check noise (IP blocks) | Likely on GitHub runners | False alarms | Two failures in a row before an issue | Self-hosted runner if needed |

**In short:** the common breakages (player JS, client churn) cost speed,
not downloads, because yt-dlp sits behind dm's reader. The breakages that
fail downloads (PO tokens, SABR-only) fail yt-dlp too. A health check
running dm's reader alone is what tells us our port fell behind before any
user would notice.
