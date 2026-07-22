# Sprint 016 — one contract source: shared structs, schemars schemas, ts-rs types

Proposal `korg:556` — the fourth bundle (B4) of the 2026-07 deep-review cleanup
(`sprints/review/REVIEW.md`). Covers WIs #539–#542, closing findings F-22, F-15
and F-23, and implementing decision D-4 (full generation). Prerequisite B3
landed in sprint 015, which is what made the shapes final enough to encode.

F-22 was named "the structural cause": every other drift finding in the review
was a symptom of the same operation shape being written by hand five times —
a patch struct in korg-core, a request struct in korg-api, an arg struct in
korg-mcp (each with its own `double_option`), a `json!` schema literal in the
tool table, and a TypeScript interface in `web/src/lib/api.ts`. Adding a field
meant five edits and hoping. This sprint reduces that to one edit.

## One definition per operation (#539)

There is no separate "shared request struct" layer, because there did not need
to be one: the core `New*`/`*Patch` types **are** the serde types now.
`NewWorkItem`, `WorkItemPatch`, `NewCard`, `CardPatch`, `NewLink`, `LinkPatch`,
`NewProposal`, `ProposalPatch`, `ProjectPatch`, `NewTopic`, `TopicPatch` and
`NewReport` derive `Deserialize`, and both transports deserialize them
directly. Operations that never had a core struct to live on — comment bodies,
`relate`, the daily-plan verbs, the collection filters — live in the new
`korg_core::ops` module.

That collapses the field-by-field mapping too. A REST handler is now the whole
operation:

```rust
async fn create_card(State(s): State<AppState>, Json(b): Json<NewCard>) -> ApiResult {
    Ok(Json(json!(repo::create_card(&s.pool, b).await?)))
}
```

`korg-api/src/lib.rs` went from 1236 lines to 760; `korg-mcp/src/tools.rs`
from 1518 to 656.

Three `double_option` implementations (`deser_nullable_str`,
`deser_nullable_i64`, `double_option`) became one, in `ops`.

Conversions that both transports did by hand moved into the type: `rank`
deserializes straight to `Decimal`, and `report_date` straight to `time::Date`,
instead of arriving as `f64`/`String` and being converted twice with two
different error messages.

**MCP carries the target id in the argument object; REST carries it in the
path.** Rather than reintroduce a wrapper struct per tool, MCP deserializes the
*same* object twice — once into a tiny id selector (`ops::NodeId`,
`ops::WiNumber`, `ops::Id`), once into the shared body. Neither declares
`deny_unknown_fields`, so each pass ignores the other's key. This avoids
`serde(flatten)`'s buffering caveats entirely.

## Schemas derived, not written (#540)

`tools()` has no `json!` literals left. Each descriptor names its argument
types and `schemars` produces the schema:

```rust
tool2::<ops::NodeId, CardPatch>("update_card", "…")
```

The enum lists come from `korg_core::vocab` through `schema_with` builders, so
a vocabulary change reaches `tools/list` with no hand-edit at all — that is the
half of F-22 that was actually causing incidents.

rmcp did not resist derivation, so the parity-test fallback was not needed as a
substitute. It ships anyway, alongside a committed snapshot of the full 44-tool
surface (`crates/korg-mcp/tests/tools_schema.json`) compared name-by-name, and
a check that every advertised tool has a dispatch arm.

### The schema diff, field by field

The generated output was diffed against the hand-written schemas. Everything
that changed is a truthfulness fix; nothing agents relied on was removed:

- **`Option<T>` fields now advertise `null`** — `project_id`, `area_id`,
  `content`, `description`, `summary`, `archived`, `pinned`, `read`,
  `machines`, `deploy_to`, `source_node_id`. Serde has always accepted an
  explicit `null` there and treated it as absent; the schema said otherwise.
- **`tags` advertises `"default": []`**, which is what it does.
- **Two fields gained descriptions** (`work_item_numbers`,
  `finding_work_items`) from the doc comments on the shared struct, and
  `search_topics.q` picked up the description `list_topics.q` already had.

Three regressions the first generated pass introduced were fixed rather than
accepted: `limit`/`offset` lost their advertised defaults (schemars overwrites
`schema_with`'s default with serde's, which is `null` for an `Option`; the
documented values are wired back through `#[schemars(default = …)]`),
`rank`'s default serialized as the string `"0"` (it is a `Decimal`), and
`list_daily_plan` briefly advertised a `source_node_id` it ignores.

`additionalProperties: false` is pinned in normalization rather than derived
from `deny_unknown_fields`. Deriving it would have made the server reject
unknown fields — a real behaviour change, and one that would also break the
two-pass id/body parse. The schema stays exactly as advisory as it has always
been; this is now written down in `docs/api.md` instead of merely being true.

## TypeScript generated (#541)

`ts-rs` emits `web/src/lib/generated/korg.ts` from the response rows, and a
small generator in `vocab.rs` emits `web/src/lib/generated/vocab.ts` with the
vocabularies and the relationship-label registry. Both run under
`cargo test export_bindings`, so one `just gen` produces the whole directory.

`web/src/lib/api.ts` went from 592 lines to 325 and now contains fetch wrappers
and nothing else. The drift it was carrying:

- `WorkItem.wi_status` was typed `string` while `Card`/`Link`/`Proposal` used
  unions — an inconsistency that hid the next one:
- `WI_TYPES` listed nine values (`idea`, `issue`, `epic`, `story`, …) of which
  **the server rejects six**. The generated list is the seven `vocab::WI_TYPES`
  actually accepts.

Client type names now match the Rust ones (`WorkItemRow`, `CardRow`,
`ProjectRow`, `LinkRow`, `ProposalRow`), so there is one name per concept
rather than a rename layer that would itself need maintaining. `i64` maps to
`number`, not ts-rs's default `bigint`, because korg serializes it as a JSON
number.

Dead surface deleted (F-23): `api.reorderDailyPlan`, `api.createProposal`,
`prettyDuration()`. `PROPOSAL_STATUSES` is no longer dead — the planning page
imports the generated one. `api.createProject` was on the review's dead list
but is used by two pages, so it stays.

## web/src/lib/domain.ts (#542)

One home for the presentation rules the pages were re-deriving. `grep` finds no
`"closed"`, `"Cut"` or `"related-to"` literal outside `domain.ts` and the
generated vocab, and no second `midRank`/`kindLabel` definition.

The free-text relationship-label input is now a select over the registry
(`covers`, `finding`, `depends_on`, `related-to`) plus a `custom…` escape
hatch, because korg does accept any label. Chip classes are one definition per
kind — project, category, tag — replacing the two-to-three visual variants each
had per page.

### The /plan terminal-status question — decision needed

The review flagged /plan's `TERMINAL = {done, closed, resolved}` against every
other page's `"closed"`-only check as a live inconsistency, and asked whether
the broader set was intentional.

**It was, and the two are not the same rule.** They answer different questions:

- *"Should a listing show this?"* → `isHiddenByDefault(status)` — `closed`
  only, because `done` is terminal but deliberately still visible.
- *"Does this still block what depends on it?"* → `isSatisfied(status)` —
  `resolved`, `done`, `closed`, because a `resolved` item is implemented and
  its dependents can proceed. This is /plan's question, and the comment above
  its old `TERMINAL` set already said so.

So `domain.ts` exports both, named for what they answer, and behaviour on every
page is unchanged. Collapsing them into a single `isTerminal()` as the work
item's acceptance criterion literally proposed would have silently changed
/plan's meaning — either items awaiting a PR would start reading as blockers,
or `done` items would vanish from listings. **Ken: overrule this if you
disagree**; it is a one-line change in `domain.ts` either way.

## just gen and the freshness fence

```
just gen      # regenerate the TypeScript and the tool-schema snapshot
just check    # fmt · gen freshness (git diff --exit-code) · clippy -D warnings · tests
```

The ts-rs export directory and the `i64 -> number` mapping live in
`.cargo/config.toml` rather than in the recipe. They have to: ts-rs reads them
at compile time, so a plain `cargo test --workspace` — which runs the generated
`export_bindings_*` tests too — would otherwise write a second, stale copy into
`crates/korg-core/bindings/` and leave the tree dirty. Putting them in cargo's
`[env]` with `relative = true` also fixes ts-rs resolving a relative export dir
against the *crate* root instead of the workspace root.

Documented in `docs/setup.md`; `docs/api.md` gained a normative "where the
contract lives" section, including the two things deliberately *not* shared
(query-string filters, whose encodings genuinely differ, and the advisory
`additionalProperties`).

## Verified

- `just check` green end to end: `cargo fmt --check`; `just gen` leaves the
  tree clean; `cargo clippy --workspace --all-targets -D warnings` silent;
  `cargo test --workspace` **all passing** (the suite grew by five `ops` unit
  tests, the vocab generator, and three schema tests).
- Two of those `ops` tests exist because the change they cover had no fence:
  `report_date` now deserializes straight to a `Date` where each transport used
  to parse a `String` by hand, and **nothing exercised the MCP `create_report`
  path at all** — it still has no integration test, only this serde fence.
  `rank` likewise moved from `f64`-plus-conversion to `Decimal` on the wire.
- `pnpm check` (323 files, 0 errors), `pnpm lint` clean.
- Playwright **27 passed**. One pre-existing failure:
  `slot-schedule.spec.ts` ("drag a card into today's daily plan"). Confirmed
  pre-existing by stashing this branch and running the spec against `main`,
  where it fails identically — it is Playwright's HTML5 `dragTo`, not this
  work. Not fixed here; worth a work item.
- `work-item-edit.spec.ts` was updated for the new label picker (select
  `custom…`, then type the label) — the only e2e change this sprint needed.
- No production access at any point; all verification ran against scratch
  containers, which were destroyed afterwards.

## Notes for what follows

- **The handoff plan can start now**, which was the point of sequencing it
  after this: handoff request/response types will be born generated, so the
  new entity costs one struct instead of five hand-synced copies.
- **B5** (docs and ops) can drop its "drift tests for tool count/routes" item
  down to a formality — the snapshot test covers the tool surface, and `just
  check` is the `just check` that item asked for.
- The generated `korg.ts` types a row's `status` as plain `string`, because
  that is what the DB column is. The two places the UI needs the union
  (`board[status]`, the edit form) cast at the boundary with a comment. If that
  spreads, the answer is real enums in korg-core, not more casts.
