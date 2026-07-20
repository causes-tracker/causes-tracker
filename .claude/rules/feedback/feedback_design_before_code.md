# Settle the design before coding; check the shape before review

For any change touching contracts, traits, crate boundaries, or test strategy: discuss and settle the design first, then implement, then verify the result is the clean expression of that design — all before asking for review.

**Why:** PR #409's db layer took many review rounds (helper splits, trait shape and placement, verb naming, capability bounds, validation placement, test scope) that were design discussion happening after code was pushed; each round was avoidable.

**How to apply:** present the shape — what exists, what gets deleted, the tradeoffs — and get agreement before writing it.
After implementing, re-derive the design from the final code and ask of each element whether it would survive the design discussion; fold or delete what would not.
Properties the type system can carry should not be carried by tests; tests cover only what types cannot express.
[[feedback_review_0_before_push]] is the surface pass; this is the shape pass.
