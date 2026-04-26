# api\_db\_macros

Proc macros for the [`api_db`](../api_db) crate.

Currently exposes one entry point — `journal_table!` — which generates the per-resource journal-table machinery (`insert_entry` and `entries_since`) from a small declarative spec.

## journal\_table!

Each resource type (Plan, Sign, Symptom, Comment, …) has its own journal table.
The 14 meta columns and replication-serving columns are identical across all of them; only the table name and payload columns differ.
This macro collapses the boilerplate so a resource module is mostly its struct declaration plus one macro invocation:

```rust
pub struct Plan {
    pub header: JournalEntryHeader,
    pub meta: ResourceEntryMeta,
    pub title: PlanTitle,
    pub description: Markdown,
    pub status: PlanStatus,
}

api_db_macros::journal_table! {
    table = "plan_journal",
    rust = Plan,
    payload = {
        title: PlanTitle,
        description: Markdown,
        status: PlanStatus,
    },
}
```

The macro emits two functions in the calling module:

- `pub async fn insert_entry(pool: &DbPool, entry: &Plan) -> anyhow::Result<()>`
- `pub async fn entries_since(pool, project_id, after_cursor) -> anyhow::Result<Vec<JournalStoredEntry<Plan>>>`

Both wrap `sqlx::query!` with literal SQL composed from the table name and payload columns at expansion time, so compile-time SQL/type validation is preserved.
Payload field types must implement `JournalText` (defined in `api_db::journal`); `String` has a blanket impl, project newtypes opt in by providing the `String ↔ Self` conversion they want.

Author handling is currently hardcoded to `LocalId::User` — every existing journal table stores `author_local_id` as a bare UUID.
The macro will gain an `author = …` knob when journal tables grow a discriminator column.
