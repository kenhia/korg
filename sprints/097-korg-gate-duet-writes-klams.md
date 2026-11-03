# 097 — korg-gate: Duet writes klams, four knowledge writes

Proposal korg:4326, slice 2 of program korg:4327 ("Claude Duet writes klams").
This is the **code half** of WI 4324, scoped by Ken's ruling of 2026-11-02
(given in the overseer session on cleo, answering claude-duet WI 4307's open
questions). The ops half is korg:4308 (claude-duet): the identity's scopes, the
deploy to krgate, the connector and the phone tests. Nothing was deployed to
krgate. It follows sprint 095's pattern. This sprint ran as a karc leg on kai
(`korg-ec95eb`), under oversight.

## Premise check

- **WI 4324 holds.** The klams door was read-only. `KLAMS_REFUSED_TOOLS` held
  all seven klams writes, `Door::allows` for klams read only
  `KLAMS_READ_TOOLS`, and the door's three human-facing strings, plus its
  refusal message, said read-only.
- **The catalogue was not full scope, as the item suspected.** The committed
  `tests/klams_tools.json` had 11 tools, the set a `claude` (read, write) identity
  sees. klams's `required_scope` names six more at `Admin`:
  `memory_admin_restore`, `_hard_delete`, `_list_deleted`, `_list_authors`,
  `_remove_author` and `_merge_authors`. klams filters `tools/list` by
  scope, so the forcing test had never seen them.
- **Slice 1 (klams WI 4323) had not shipped.** It was mid-flight on kubs0
  (branch `057-bound-author-writes`), so I took the catalogue from klams
  `main` (`8ae5c7a`, klams-mcp 0.1.55). Today `memory_add`'s `author_id`
  description still reads "Pass explicitly to write as a different registered
  author". The forcing test compares names only, so slice 1's description
  change does not break it. Refresh the fixture after slice 1 ships so it stays
  current. That is a refresh, not a ruling.
- `memory_add`'s `kind` is a required enum, `fact | knowledge`. `author_id`
  defaults to the nil UUID. `memory_supersede` and `memory_update` declare no
  `author_id`. `memory_delete` does. `event_search` takes `author_id` as a
  **filter**, which matters below.

## Decisions

- **Three lists, as on the korg door.** `KLAMS_WRITE_TOOLS` (4, sorted) sits
  between `KLAMS_READ_TOOLS` and `KLAMS_REFUSED_TOOLS`. The refused list is now
  `dissent_propose`, `memory_append_event`, `register_author` and the six admin
  tools, 9 in all. The admin tools go in REFUSED because the item said so
  unless the overseer ruled otherwise. `klams_tool_allowed` is read or write.
  The forcing test is total over the three. Unit tests keep them sorted and
  disjoint, and an integration test pins `KLAMS_WRITE_TOOLS` to the four, with
  a message saying widening it is a decision.
- **The full-scope catalogue came from klams's source, not a live call.** No
  live identity holds `admin`. A `ken` identity, which klams's docs use as the
  example, returns 401 "unknown identity". Minting an admin identity on kubs0
  would be an ops change to another repo's auth, made only to read a list. So
  I built an example in a throwaway klams clone (`.scratch/097/klams-src`)
  that prints `klams_mcp::tools::all_tool_descriptors()` (17 tools). I checked
  it first: the descriptors for the 11 tools a `claude` identity sees equal the
  live `tools/list` **byte for byte**. The old fixture also equalled the live
  list. Written in klams's own order with the fixture's formatting, the diff is
  purely additive (+153 lines). A new test,
  `the_klams_catalogue_is_a_full_scope_view`, fails if the fixture carries no
  `memory_admin_*` tools, so a future refresh from a write token cannot quietly
  narrow it again. `docs/gate.md` and the fixture's doc comment give the
  refresh procedure.
- **`author_id` is stripped, never stamped.** `strip_klams_author` runs on
  klams-door `tools/call` for the four writes only. It removes
  `arguments.author_id`. Missing or `null` arguments become `{}`, and
  arguments that are not an object are refused with `-32602`. That is the
  `stamp_origin` discipline, applied to the same re-serialised message. Reads
  are untouched: `event_search`'s `author_id` filter passes as sent, and a
  test pins that, because stripping it would quietly change a read.
- **`memory_add` is refused unless `kind` is exactly `"knowledge"`.** A
  missing kind, `null`, `"fact"` and `"Knowledge"` are all refused. The check
  is a string equality, not a case fold. The refusal is `-32602`, like an
  unlisted tool. Its message gives the reason: "memory_add through korg-gate
  writes knowledge only: `kind` must be "knowledge"".
- **The scope keeps its name, `klams:read`.** This is the same option-A
  reasoning as 095: claude.ai has no scope picker, and a new scope would force
  a re-sign-in while separating nobody. `docs/gate.md` says so.
- **The klams door's wording was corrected as 095 round 2 corrected korg's.**
  `resource_name` is now `klams` (was `klams (read-only)`). The sign-in page
  reads "wants access to klams's reads and the ruled write set", and the
  button reads **Allow access** (was **Allow read access**). The refusal now
  reads "exposes klams's reads and the ruled write set". `sign_in_button` is
  now one string for both doors.

## What shipped

- `crates/korg-gate/src/allowlist.rs`: `KLAMS_WRITE_TOOLS`, the narrowed
  `KLAMS_REFUSED_TOOLS`, `KLAMS_KNOWLEDGE_KIND`, `KLAMS_AUTHOR_FIELD` and
  `klams_tool_allowed`. The unit test now covers three lists.
- `crates/korg-gate/src/door.rs`: klams `allows` is read or write; the
  wording above.
- `crates/korg-gate/src/proxy.rs`: `strip_klams_author`; the per-door rewrite
  dispatch; the WARN line.
- `crates/korg-gate/tests/klams_tools.json`: regenerated as the full-scope view
  (11 → 17).
- Tests (`tests/allowlist.rs`):
  - `every_klams_tool_has_been_ruled_on_and_every_ruling_names_a_real_tool`
    covers three lists, with both pins.
  - `the_klams_catalogue_is_a_full_scope_view`.
  - `every_refused_klams_tool_is_refused_on_call_and_the_list_is_reads_plus_writes`:
    11 refused, made up of the 9 plus a korg read and a korg write. None
    reaches the stub. The list is exactly the 8.
  - `every_klams_write_reaches_klams_with_author_id_stripped`: a foreign UUID
    is sent on all four, and the stub never sees it. The other arguments are
    intact, and `X-Homelab-Agent: claude-duet` is on each request.
  - `memory_add_is_knowledge_only_and_unstrippable_arguments_are_refused`:
    five bad `memory_add` shapes and two non-object argument cases. None
    reaches the stub. The refusal says why, and a missing or `null` argument
    set forwards as `{}`.
  - `a_klams_read_keeps_its_author_id_filter`.
  - `tests/flow.rs` pins the new klams strings, and that the klams page no
    longer says "read-only". The SSE unit test's refused example moved from
    `memory_add` to `memory_append_event`.
- `docs/gate.md`: the klams write set, the refusals, the strip rule, what
  `manage` means (Duet can soft-delete any record; restore needs `admin`,
  which this door refuses), the full-scope fixture procedure, and a "Turning
  the klams writes on" section for the ops half. Its order is slice 1, then
  the identity's scopes, then the binary, then the connector and approval,
  then the phone tests. README and crate docs no longer call the klams door
  read-only.

## Verification

- `just check` green: 765 tests, 0 failed.
- **Mutation checks.** I removed the `arguments.remove(author_id)` line, and
  `every_klams_write_reaches_klams_with_author_id_stripped` failed. I disabled
  the kind check, and
  `memory_add_is_knowledge_only_and_unstrippable_arguments_are_refused`
  failed. Both were restored.
- **Survey against the previous binary**, as 095 did. On kai, I built `main`
  at `1990b09` in a throwaway worktree with its own target dir (sha256
  `dcdc6435…`) and built this branch (`ff83d533…`). The old binary has the
  string `klams (read-only)` and lacks the new refusal wording; the new binary
  is the reverse. Both ran on `127.0.0.1:18475` with **one state directory**,
  against live korg-api (kubsdb:5674) and live klams (kubs0:7777). The script
  is `.scratch/097/smoke097.py`, 095's script with a stable klams refusal
  probe and live klams probes added. I probed from kai, the host running both
  gates.
  1. **Unauthenticated survey, 22 rows.** 20 are identical, including all four
     korg rows (both metadata forms and both sign-in pages). The two that
     differ are the klams metadata and the klams sign-in page, with status and
     headers identical. The only differences are `resource_name`
     (`klams (read-only)` → `klams`), the sentence and the button.
  2. **Authenticated.** The old gate's korg and klams refresh tokens both
     refreshed on the new gate (200, `korg:read` / `klams:read`,
     `expires_in 3600`). These were identical across the two binaries: korg
     `tools/list` (34 → 34, nothing added or removed), korg's
     `update_proposal` refusal, korg's method refusal, korg's
     `server/discover`, and klams `tools/list` (4 → 4, because live klams
     still narrows `claude-duet` to the four reads until the ops half widens
     its scopes). The two wrong-door 401s print byte-identical, and the
     script reported them as differing only because of 095's list/tuple
     artefact. The klams refusals of `add_comment` and `memory_append_event`
     differ only in the wording above. The old `memory_add` refusal is now
     the knowledge-only refusal, which is intended.
  3. **Live klams through the new gate.** `memory_add kind:"fact"` was refused
     at the gate and never reached klams. `memory_add kind:"knowledge"` with a
     foreign `author_id` was forwarded, and klams answered
     `INSUFFICIENT_SCOPE` ("requires scope Write"). So nothing was stored, and
     klams's own scope check is still the backstop until the ops half.
     `memory_delete` was forwarded and got the same answer.
     `memory_append_event` was refused at the gate. The journal had a
     `tools/call … door="klams"` line for each forwarded call and the WARN
     line for the refused one.
  The survey ran before one change: adding `tool` to the new WARN line, which
  is a log field and nothing else. `just check` ran after it.
- **The korg door is unchanged.** Beyond the survey, no korg list, string or
  test was touched. `git diff` on `allowlist.rs` changes only the klams
  section and the module doc.

## Cross-repo changes made

- **cross-project-planning `7fc4971`** (main, pushed, `main...origin/main`,
  no open PRs). It appends the klams-writes decision to GP-17 in
  `korg+/PLAN.md`. The text names the four tools, knowledge-only `memory_add`,
  the refusals, the `author_id` strip, slice 1 as the second control,
  `manage` and soft delete, approval with WI 4309 open, and the scope's name.
  It says the second door's "klams refuses `claude-duet`'s writes on its own"
  is replaced once the ops half lands, and it ends with "A wider set on either
  door, or a third door, is still a new decision". This is the same inline
  procedure as `05b7fff`. The claude-duet repo was not touched; its docs
  belong to the ops half.

## Repaired in passing

- **A refused argument rewrite left no log line.** Before this change, a
  korg-door `tools/call` that could not be stamped was refused silently. Now
  either door logs a WARN for it (`tools/call arguments refused … door=…
  tool=…`), right after the `tools/call` INFO line, so the journal shows the
  call and then its refusal. It is log-only, and the survey shows korg-door
  responses unchanged. The overseer ruled it a repair in passing (korg:4326
  comment 4296, ruling 3).

- **`deploy/korg-gate/korg-gate.service` still said read-only** (the nit 095's
  ops half recorded and did not file). Its header comment and `Description=`
  now say "OAuth front door to korg and klams MCP". The same stale wording was
  fixed in `env.example`'s klams line and the crate's `Cargo.toml`
  description. All are display strings. They take effect on the next unit
  install in the ops half. Proof: `just check` and a grep for
  `read-only` under `crates/korg-gate`, `deploy/` and `docs/gate.md` (only
  historical mentions remain).

## Overseer rulings (korg:4326 comment 4296)

1. **Accepted:** the full-scope catalogue taken from klams source. Minting an
   admin identity only to read the list would have been an auth change in
   klams, which is outside this slice.
2. The fixture refresh after slice 1 goes into korg:4308's notes. The test
   only checks names, so it does not hold this ship.
3. **Accepted:** the WARN line, as a repair in passing (above).

## Follow-ups

- None filed. The fixture refresh after slice 1 is noted above. It is a
  name-preserving description refresh, and slice 1's own item already asks for
  a note to this leg on any description diff.
