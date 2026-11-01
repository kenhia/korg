# 095 — korg-gate: Duet writes korg, the scheduling set

Proposal korg:4298, slice 3 of program korg:4299 ("Claude Duet front door,
phase 2"). This is the **code half** of WI 4244, scoped by Ken's ruling of
2026-10-31 (WI 4244, comment 4264). The ops half (deploy to krgate, the
per-tool approval check, a real comment and proposal from the phone) runs
interactively with Ken after this ships, and the proposal stays `active` for
it. Nothing was deployed to krgate. This sprint ran as a karc leg on kai
(`korg-d3e643`), under oversight.

## Premise check

- **WI 4244 holds.** `READ_TOOLS` was the korg door's whole list, and the
  forcing test only noticed read-shaped names, so a new write would have been
  silently refused rather than ruled on. korg's catalogue has 57 tools: 26
  reads, the 8 ruled writes, and 23 others. Those 23 are exactly the ruling's
  refusal classes, with nothing left over.
- **"The two korg tools that carry `origin`" holds, with one more on the
  refused side.** `add_comment`, `relate` and `update_comment` have an `origin`
  property. `update_comment` is refused, so the stamp is needed on the two
  writes the brief names. `set_awaiting`'s schema mentions the word only in
  prose.

## Decisions

- **The ruling is total, as klams's is.** `WRITE_TOOLS` (8) and
  `REFUSED_TOOLS` (23) sit next to `READ_TOOLS` in `allowlist.rs`. Every tool
  in `crates/korg-mcp/tests/tools_schema.json` must be in exactly one of the
  three, and every name in the three must exist in the catalogue. A
  read-shaped name must still be a read. The test also pins `WRITE_TOOLS` to
  the eight names, with a message that says widening it is a decision.
- **`origin` is overwritten at the gate, not trusted.** On `add_comment` and
  `relate` (`ORIGIN_TOOLS`), the gate sets `arguments.origin = "claude-duet"`
  in the message it re-serialises, whatever the caller sent. If `arguments`
  is missing or `null`, the gate creates an object. If it is anything else,
  the call is refused with `-32602`, so an unstampable call never reaches
  korg. No other field and no other tool is touched; `create_work_item` with
  an `origin` key forwards it as sent, since that is not korg's field there.
  `X-Homelab-Agent: claude-duet` stays on every forwarded request.
- **OAuth scope: option A.** The single `korg:read` scope keeps its name. The
  allowlist and claude.ai's per-tool approval are the controls on writes. Why
  not `korg:write` (option B): there is one owner, one passphrase and one
  client, so a second scope would separate nobody. claude.ai's connector flow
  has no scope picker, so every existing token would lack the new scope and
  the phone would have to be disconnected and signed in again. That would buy
  nothing a scope check could add. **The scope's name is now inaccurate, on
  purpose,** and `docs/gate.md` says so. Round 1 also kept two human-facing
  read-only strings byte for byte and flagged them. The overseer then ruled
  them a repair in this change (see "Repaired in passing").
- **The korg door's refusal message changed, and klams's did not.** It said
  "which exposes korg read-only", which is now false. It now says "which
  exposes korg's reads and the ruled write set". It is a JSON-RPC error body
  to a refused call, not metadata, sign-in or token responses. klams's reads
  "exposes klams read-only" as before, and the survey below shows it identical.
- **`Door::tools()` is gone.** The korg door is now two lists, so `allows`
  matches on the door instead. `tool_allowed` is the korg door's check (reads
  or writes), and `Door::exposes()` carries each door's refusal wording.

## What shipped

- `crates/korg-gate/src/allowlist.rs`: `WRITE_TOOLS`, `REFUSED_TOOLS`,
  `ORIGIN_TOOLS`, `DUET_ORIGIN`; `tool_allowed` admits reads and writes; a
  unit test keeps the three korg lists sorted and disjoint, and requires
  `ORIGIN_TOOLS` to be in `WRITE_TOOLS`.
- `crates/korg-gate/src/door.rs`: `allows` per door; `exposes`.
- `crates/korg-gate/src/proxy.rs`: `stamp_origin` on korg-door `tools/call`;
  the refusal wording; the `server/discover` INFO line.
- `crates/korg-gate/src/main.rs`: the startup line's `upstreams` field.
- Tests (`tests/allowlist.rs`): `every_korg_tool_has_been_ruled_on`,
  `every_ruled_tool_exists_in_the_catalogue`,
  `every_read_shaped_tool_in_the_catalogue_is_a_read`,
  `the_write_set_is_exactly_the_ruled_eight`;
  `every_refused_tool_is_refused_on_call_and_the_list_is_reads_plus_writes`
  (34 names, none of the 23 reaches the stub);
  `every_write_tool_reaches_korg_stamped_as_duet`, which sends
  `origin: "overseer"` on all eight and asserts that the stub saw
  `claude-duet` on `add_comment` and `relate`, `overseer` on the other six,
  the other arguments untouched, and `X-Homelab-Agent` on each; and
  `origin_is_stamped_when_the_caller_sends_none_and_refused_on_non_object_arguments`.
  The pre-094 refresh-token test in `tests/flow.rs` now expects 34 tools.
- `docs/gate.md`: the write set, the refusal list, the origin rule, the
  per-tool-approval condition, the scope decision and its inaccuracy, and a
  "Turning the writes on" section with the ops-half steps. `README.md`'s
  korg-gate bullet no longer says read-only. The crate docs are updated to match.

## Verification

- `just check` green (below).
- **Mutation check.** Removing the one line that writes `origin` fails both
  origin tests (`add_comment` first), restored afterwards.
- **The unchanged parts of the korg door, proved against the previous
  binary**, as 094 did. On kai, I built `main` at `33a388e` (in a throwaway
  worktree with its own target dir, sha256 `2469d27b…`) and this branch
  (`5d3ca654…`). I confirmed the old binary lacks the new refusal string and
  the new one has it. Both ran on `127.0.0.1:18475` with **one state
  directory**, against live korg-api (kubsdb:5674) and live klams
  (kubs0:7777). The script is `.scratch/smoke095.py`, 094's `smoke.py` extended
  to the klams door.
  1. **Old binary, unauthenticated survey:** 22 responses. These were: four
     metadata documents (korg root, korg path-inserted, klams, AS); the 401 on
     POST/GET/DELETE for both doors; a 405; healthz; an evil registration;
     four sign-in pages (no resource, korg's, klams's, a bad one); three token
     errors; and a bogus bearer at each door. Status, headers (minus `date`
     and `content-length`) and body were recorded.
  2. **Old binary, sign-in** at both doors. It recorded the refresh tokens,
     korg's 26-tool list, korg's `update_proposal` refusal, korg's
     `resources/list` refusal, klams's tool list and two klams refusals, each
     wrong-door 401, and korg's `server/discover` answer.
  3. **New binary, same state directory.** The survey was **identical**
     (`diff` empty, 22 rows). The old gate's korg refresh token refreshed
     (200, `korg:read`, `expires_in 3600`), and so did its klams one
     (`klams:read`). These were identical: klams `tools/list`, both klams
     refusals, korg's method refusal and korg's `server/discover`. The two
     wrong-door 401s printed byte-identical JSON. The script's `==` reported
     them as differing only because it compared a JSON list with a Python
     tuple. korg's `tools/list` went from 26 to 34. It added exactly the eight
     and removed none. The `update_proposal` refusal differs only in its
     wording (above).

     **After round 2, rows 1, 2, 14 and 15 of the 22 differ, and that is
     intended.** They are the two korg metadata forms and the two korg sign-in
     pages (no `resource`, and korg's). I rebuilt the binary and re-ran the
     unauthenticated survey only (the overseer ruled the two-binary run
     unnecessary), then diffed it against the old binary's stored rows. The
     metadata rows differ only in `resource_name` (`korg (read-only)` →
     `korg`). The sign-in rows differ only in the sentence ("wants
     **read-only** access to korg" → "wants access to korg's reads and the
     ruled write set") and the button (**Allow read access** → **Allow
     access**). The other 18 rows are still identical, including klams's
     metadata and sign-in page. So the korg door's intended differences from
     `33a388e` are four: the 26 → 34 tool list, the refusal wording, the
     `resource_name`, and the sign-in sentence and button.
  4. **One live write through the new gate:** `add_comment` on korg:4298 with
     `origin: "smoke-sent-this"`. korg stored comment 4267 with origin
     `claude-duet`, read back through the gate with `list_comments`. Its body
     says it was the loopback smoke, not Duet.
  5. The new binary's journal opened with
     `upstreams="korg=https://kubsdb…:5674/mcp klams=https://kubs0…:7777/mcp"`
     and logged `forwarded … door="korg" method="server/discover"`, then a
     `tools/call` line for each of the two calls.

## Cross-repo changes made

- **cross-project-planning `05b7fff`** (main, pushed, `main...origin/main`).
  It appends the korg-writes decision to the GP-17 exception paragraph in
  `korg+/PLAN.md`. The paragraph names the eight tools, the refusal classes,
  the origin overwrite, per-tool approval and the scope's name, and ends "A
  wider set, or a klams write, is still a new decision." It follows the same
  inline procedure as `7e83eed` and `72466fa`. I did not edit the claude-duet
  repo; its `docs/rulings.md` line belongs to the ops half.

## Repaired in passing

The first two were ruled on in the ops half of 4297 (WI 4294, comment 4262).
The third was the overseer's round-1 ruling on comment 4268.

- **The startup log named only korg's upstream.** It now carries
  `upstreams="korg=… klams=…"`, built from the open doors. Seen in the new
  binary's first journal line (Verification 5).
- **A forwarded `server/discover` left no INFO line**, unlike `tools/call`.
  It now logs `forwarded client=… door=… method="server/discover"`. Seen in
  the same journal (Verification 5).
- **korg's door still told humans it was read-only.** This changes no
  behaviour; the strings are display only. `resource_name` is now `korg`.
  The sign-in page now reads "wants access to korg's reads and the ruled
  write set". I also changed the button from **Allow read access** to
  **Allow access**. The ruling named two strings, but the button is a third
  false statement on the same page, and the ruling's reason covers it, so I
  repaired it rather than leaving it. All of klams's strings are unchanged.
  `tests/flow.rs` pins the new korg strings (and that the korg page no longer
  says "read-only") and klams's old ones. The proof is the four-row survey
  diff (Verification 3) and `just check`.

## Follow-ups

- None filed.
