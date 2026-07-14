# buck2 client errors mask daemon deaths

When the buck2 daemon dies (e.g. panics), the client reports only the event-bus collapse: "h2 protocol error: error reading a body from connection: broken pipe".
That message describes the client↔daemon stream, not the real failure, and looks identical for any daemon death.
The daemon's stderr (panic message and backtrace) is at `~/.buck/buckd/<repo-path>/<isolation-dir>/buckd.stderr` (default isolation dir `v2`; previous daemon under `prev/`).

**Why:** chasing the client message as a network error cost two days against BuildBuddy; the real cause was a daemon panic visible only in daemon stderr.

**How to apply:** on any buck2 "broken pipe"/event-bus error, read daemon stderr before theorizing.
