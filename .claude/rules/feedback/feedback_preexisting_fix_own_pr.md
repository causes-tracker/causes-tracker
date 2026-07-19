# Pre-existing problems get their own PR, not a fold-in

A defect discovered while doing feature work, if it is independent of that work, gets its own PR targeted to master with nothing stacked on it.
Do not fold it into the current change, even when the fix is trivial or the problem only became visible because of your change.

**How to apply:** "do one thing" holds.
Fold a fix in only when it is genuinely part of the same concern; otherwise open a standalone master-targeted PR.
