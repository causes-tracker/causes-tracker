//! Proc macros for the `api_db` crate.
//!
//! Currently exposes one entry point — `journal_table!` — which generates
//! the `insert_entry` and `entries_since` functions for a per-resource
//! journal table from a small declarative spec.
//!
//! Generated code uses `sqlx::query!` against literal SQL composed at
//! macro-expansion time, so compile-time SQL validation is preserved.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token, Type, braced, parse_macro_input};

/// Declarative spec for a per-resource journal table.
///
/// ```ignore
/// journal_table! {
///     table = "plan_journal",
///     rust = Plan,
///     payload = {
///         title: PlanTitle,
///         description: Markdown,
///         status: PlanStatus,
///     },
/// }
/// ```
///
/// Generates two `pub async fn`s in the calling module:
/// - `insert_entry(pool: &DbPool, entry: &T) -> anyhow::Result<()>`
/// - `entries_since(pool, project_id, after_cursor) -> anyhow::Result<Vec<JournalStoredEntry<T>>>`
///
/// Both wrap `sqlx::query!` with literal SQL composed from the table name
/// and payload columns, so compile-time SQL/type validation is preserved.
/// Payload field types must implement `JournalText`.
///
/// Author: hardcoded to `LocalId::User` for now — every resource table
/// stores `author_local_id` as a bare UUID.  When we add a discriminator
/// column, the macro gains an `author = ...` knob.
#[proc_macro]
pub fn journal_table(input: TokenStream) -> TokenStream {
    let spec = parse_macro_input!(input as JournalTableSpec);
    expand(spec).into()
}

// ── Parsing ─────────────────────────────────────────────────────────────

struct JournalTableSpec {
    table: LitStr,
    rust: Ident,
    payload: Vec<PayloadField>,
}

struct PayloadField {
    name: Ident,
    ty: Type,
}

impl Parse for JournalTableSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut table: Option<LitStr> = None;
        let mut rust: Option<Ident> = None;
        let mut payload: Option<Vec<PayloadField>> = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "table" => table = Some(input.parse()?),
                "rust" => rust = Some(input.parse()?),
                "payload" => {
                    let content;
                    braced!(content in input);
                    let mut fields = Vec::new();
                    while !content.is_empty() {
                        let name: Ident = content.parse()?;
                        content.parse::<Token![:]>()?;
                        let ty: Type = content.parse()?;
                        fields.push(PayloadField { name, ty });
                        if content.peek(Token![,]) {
                            content.parse::<Token![,]>()?;
                        }
                    }
                    payload = Some(fields);
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown journal_table! key: `{other}`"),
                    ));
                }
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            table: table.ok_or_else(|| {
                syn::Error::new(input.span(), "journal_table!: missing `table = ...`")
            })?,
            rust: rust.ok_or_else(|| {
                syn::Error::new(input.span(), "journal_table!: missing `rust = ...`")
            })?,
            payload: payload.ok_or_else(|| {
                syn::Error::new(input.span(), "journal_table!: missing `payload = { ... }`")
            })?,
        })
    }
}

// ── Code generation ─────────────────────────────────────────────────────

const META_COLS: &[&str] = &[
    "origin_instance_id",
    "origin_id",
    "version",
    "previous_origin_instance_id",
    "previous_origin_id",
    "previous_version",
    "kind",
    "at",
    "author_instance_id",
    "author_local_id",
    "embargoed",
    "slug",
    "project_id",
    "created_at",
];

fn expand(spec: JournalTableSpec) -> TokenStream2 {
    let table_name = spec.table.value();
    let rust = &spec.rust;

    let payload_names: Vec<&Ident> = spec.payload.iter().map(|f| &f.name).collect();
    let payload_types: Vec<&Type> = spec.payload.iter().map(|f| &f.ty).collect();
    let payload_col_names: Vec<String> = payload_names.iter().map(|i| i.to_string()).collect();

    let insert_sql = build_insert_sql(&table_name, &payload_col_names);
    let select_sql = build_select_sql(&table_name, &payload_col_names);
    let insert_ctx = format!("inserting {table_name} entry");
    let query_ctx = format!("querying {table_name} entries");

    // Field bindings on the row returned by sqlx::query! in entries_since.
    // sqlx names the columns from the SELECT list, so each payload column
    // appears as `r.<name>` of its DB type (TEXT → String).
    let payload_decode = payload_names
        .iter()
        .zip(payload_types.iter())
        .map(|(name, ty)| {
            let row_field = format_ident!("{}", name);
            quote! {
                #name: <#ty as ::api_db::journal::JournalText>::from_journal_text(r.#row_field)?
            }
        });

    let payload_bind = payload_names.iter().map(|name| {
        quote! {
            <_ as ::api_db::journal::JournalText>::as_journal_text(&entry.#name)
        }
    });

    quote! {
        /// Insert a journal entry, encoding the meta columns via
        /// `JournalMetaParams` and the payload columns via `JournalText`.
        /// Runs in a REPEATABLE READ transaction.
        pub async fn insert_entry(
            pool: &::api_db::DbPool,
            entry: &#rust,
        ) -> ::anyhow::Result<()> {
            use ::anyhow::Context as _;
            let m = ::api_db::journal::__private::JournalMetaParams::from_entry(
                &entry.header,
                &entry.meta,
            )?;
            let mut tx = pool.begin_txn().await?;
            ::sqlx::query!(
                #insert_sql,
                m.origin_instance_id,
                m.origin_id,
                m.version,
                m.previous_origin_instance_id,
                m.previous_origin_id,
                m.previous_version,
                m.kind,
                m.at,
                m.author_instance_id,
                m.author_local_id,
                m.embargoed,
                m.slug,
                m.project_id,
                m.created_at,
                #(#payload_bind),*
            )
            .execute(&mut *tx)
            .await
            .context(#insert_ctx)?;
            tx.commit().await.context("committing transaction")?;
            Ok(())
        }

        /// Fetch entries for a project with `local_version >= after_cursor`,
        /// ordered ascending.  `None` cursor = "from the beginning".
        /// At-least-once: entries at the cursor may be re-delivered;
        /// callers dedup by federated version.
        pub async fn entries_since(
            pool: &::api_db::DbPool,
            project_id: &::api_db::ProjectId,
            after_cursor: ::std::option::Option<::api_db::journal::LocalTxnId>,
        ) -> ::anyhow::Result<::std::vec::Vec<::api_db::journal::JournalStoredEntry<#rust>>> {
            use ::anyhow::Context as _;
            // Real txids are ≥ 3; sentinel of 0 matches every row.
            let cursor_i64 = after_cursor.map(|c| c.as_i64()).unwrap_or(0);
            let rows = ::sqlx::query!(
                #select_sql,
                project_id.as_str(),
                cursor_i64,
            )
            .fetch_all(&pool.pool())
            .await
            .context(#query_ctx)?;

            rows.into_iter()
                .map(|r| {
                    // Author: hardcoded User until we add a discriminator
                    // column to journal tables.
                    let author = ::api_db::journal::FederatedIdentity {
                        instance_id: ::api_db::journal::InstanceId::from_raw(
                            &r.author_instance_id,
                        )?,
                        local_id: ::api_db::journal::LocalId::User(
                            ::api_db::UserId::from_raw(&r.author_local_id)?,
                        ),
                    };
                    let meta_row = ::api_db::journal::__private::JournalMetaRow {
                        origin_instance_id: r.origin_instance_id,
                        origin_id: r.origin_id,
                        version: r.version,
                        previous_origin_instance_id: r.previous_origin_instance_id,
                        previous_origin_id: r.previous_origin_id,
                        previous_version: r.previous_version,
                        kind: r.kind,
                        at: r.at,
                        author_instance_id: r.author_instance_id,
                        author_local_id: r.author_local_id,
                        embargoed: r.embargoed,
                        slug: r.slug,
                        project_id: r.project_id,
                        created_at: r.created_at,
                    };
                    let (header, meta) = meta_row.into_parts(author)?;
                    Ok(::api_db::journal::JournalStoredEntry {
                        entry: #rust {
                            header,
                            meta,
                            #(#payload_decode),*
                        },
                        local_version: ::api_db::journal::LocalTxnId::from_i64(r.local_version)?,
                        watermark: ::api_db::journal::LocalTxnId::from_i64(r.watermark)?,
                    })
                })
                .collect()
        }
    }
}

fn build_insert_sql(table: &str, payload: &[String]) -> String {
    let all_cols: Vec<&str> = META_COLS
        .iter()
        .copied()
        .chain(payload.iter().map(|s| s.as_str()))
        .collect();
    let n = all_cols.len();
    let binds: Vec<String> = (1..=n).map(|i| format!("${i}")).collect();
    format!(
        "INSERT INTO {table} ({cols}) VALUES ({binds})",
        table = table,
        cols = all_cols.join(", "),
        binds = binds.join(", "),
    )
}

fn build_select_sql(table: &str, payload: &[String]) -> String {
    let all_cols: Vec<&str> = META_COLS
        .iter()
        .copied()
        .chain(payload.iter().map(|s| s.as_str()))
        .chain(["local_version", "watermark"])
        .collect();
    format!(
        "SELECT {cols} FROM {table} WHERE project_id = $1 AND local_version >= $2 ORDER BY local_version",
        cols = all_cols.join(", "),
    )
}
