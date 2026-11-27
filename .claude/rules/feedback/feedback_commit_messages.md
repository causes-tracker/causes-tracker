# A commit message records why, at the moment of the change

A commit is fixed to a moment; the code it changes is not.
The tree shows, at any later time, what the code does and how.
The message is for what the tree cannot show: why the change was made then — the need, the failure, what was known.
Anything else in it earns its place by making the change clearer to a reader of the log after the code has moved on.

Absent a reason to vary, form follows the common conventions: an imperative subject of 50 characters or fewer, a blank line, a body wrapped at 72 columns.
Sentence-per-line is for content that is edited in place; a commit message is written once.

A PR lands as its one commit under a merge commit that keeps only the PR title.
The PR title is the commit subject and the PR body is the commit body, byte for byte, down to the one `Co-Authored-By` line.
The `🤖 Generated with` line the harness asks for on PR descriptions is left out.

**Why:** PRs #705–#708 went up with messages that dwelt on what the code was and how it was tested, and with PR bodies written separately from their commits (2026-11-27).

**How to apply:** ask of each sentence whether a reader of the log, with the code since changed, understands this change better for it.
Set the PR title and body from the commit message, and again after redescribing a pushed changeset.
