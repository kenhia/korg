//! The domain vocabulary, validated app-side at every write boundary (WI #526).
//!
//! These sets used to live in four places at once: Rust consts, DB CHECK
//! constraints and enum casts, hand-written MCP JSON schemas, and TypeScript
//! const-tuples. The DB copies were doing the enforcing, which meant a typo'd
//! t-shirt size or card status came back as a 500 with raw Postgres text.
//! korg-core is now the authority; the DB constraints are a backstop.
//!
//! `wi_type` was entirely free text before this (D-2). The vocabulary is the
//! union of what the live corpus actually uses plus `chore`, so nothing
//! existing is invalidated.

use crate::error::RepoError;

/// Every kind a `node` row may be — the typed half of the typed-node model.
///
/// **This is the vocabulary that was missing** (WI #870's audit, sprint 054).
/// Every other closed set in korg has lived here since #526, but node kinds
/// existed only as a SQL `CHECK` in whichever migration last touched it, plus
/// hand-written copies in two tests. Nothing tied the copies to the constraint,
/// so the copies did not grow when the constraint did: sprint 051 added
/// `schedule` (0025) and both lists stayed at seven. `get_node_preview` then had
/// no `schedule` arm for three sprints, and the fence written specifically to
/// catch that — `every_node_kind_resolves_to_a_real_title`, whose doc comment
/// asserts "every kind the `node.kind` check constraint admits" — iterated its
/// own hardcoded list and passed throughout.
///
/// The lesson is the one #526 already drew and this set was excluded from: a
/// vocabulary enumerated by hand in the places that consume it is a vocabulary
/// that drifts. `node_kinds_match_the_check_constraint` (tests/schema.rs) now
/// pins this list to the database, and the preview fence iterates it, so adding
/// a kind fails both until it is genuinely handled.
///
/// Ordered as the constraint declares them, oldest first: 0001 opened with
/// `workitem`/`card`, and each later migration appended.
pub const NODE_KINDS: [&str; 9] = [
    "workitem",
    "card",
    "link",
    "sprint_proposal",
    "report",
    "handoff",
    "program",
    "schedule",
    "attachment",
];

/// What an attachment's lifecycle position is called (sprint 056, #582/#1119).
///
/// **Derived on every read, never stored.** `pending` is an attachment with no
/// `has_attachment` edge — the paste-before-save state, and the only thing the
/// sweeper collects; `linked` is one an owner claimed. The handoff (D5)
/// describes this as a state the upload sets and the save advances, and a
/// column would have been the obvious way to hold it — but then the same fact
/// lives in two places, and the way they drift is the dangerous way round: an
/// edge written through the generic `relate()` path would leave the column
/// reading `pending`, and the sweeper would delete an image somebody's work
/// item points at. Derivation makes that unrepresentable.
///
/// Not in [`EXPORTED`]: it is a computed projection rather than a write-boundary
/// vocabulary, so nothing validates against it and no schema advertises it.
pub const ATTACHMENT_STATES: [&str; 2] = ["pending", "linked"];

/// Where each node kind is rendered in the web app: the **one** authority for
/// locator → URL (WI #1467, sprint 070).
///
/// Every kind in [`NODE_KINDS`] has a row, and `every_node_kind_has_a_route`
/// fences that — a tenth kind fails the build until it is given a page, the
/// same way [`NODE_KINDS`] made `get_node_preview`'s missing `schedule` arm
/// impossible to add silently.
///
/// **Why korg owns this rather than each consumer.** Four consumers hit the
/// gap this fills — korg #981's Awaiting lane, kfdc #993's Net Log, korg-vs's
/// resolve-by-ID and kfdc #1203's pane — and a kind → path map copied into
/// each of them is a claim about korg's vocabulary that korg grows between
/// deploys (GP-14). Consumers that hold only a node id use `/n/:node_id`,
/// which resolves the kind server-side and redirects here; consumers korg
/// already hands a node ref get the path on the ref (`NodePreview.url`,
/// `SearchHit.url`). Neither reconstructs this table. (GP-13.)
///
/// Per-kind routes rather than one generic `/nodes/:id` page: settled in
/// #621 / sprint 028, because the payloads genuinely differ and a URL that
/// says `/planning/1469` is legible where `/nodes/1469` is not. `/n/:id` is a
/// *resolver*, not a page — it redirects and holds no rendering of its own,
/// so it does not reopen that decision.
///
/// A work item's `{id}` is its `wi_number`, which since the 0009 identity
/// migration **is** its node id — so one id builds every row here and no
/// caller needs the distinction.
pub const NODE_ROUTES: [(&str, &str); 9] = [
    ("workitem", "/work-items/{id}"),
    ("card", "/cards/{id}"),
    ("link", "/reading-list/{id}"),
    ("sprint_proposal", "/planning/{id}"),
    ("report", "/daily-reports/{id}"),
    ("handoff", "/handoffs/{id}"),
    ("program", "/programs/{id}"),
    ("schedule", "/schedules/{id}"),
    ("attachment", "/attachments/{id}"),
];

/// The path that renders this node on its own page, or `None` for a kind korg
/// has no route for — which, by [`NODE_ROUTES`]'s fence, is only a kind that
/// does not exist.
///
/// Returning `Option` rather than a bare `String` keeps the "I cannot say"
/// answer representable: a caller holding a `kind` string off the wire can be
/// wrong about it, and GP-13's consumer half says such a caller renders
/// nothing rather than a path it made up.
pub fn node_path(kind: &str, node_id: i64) -> Option<String> {
    NODE_ROUTES
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, template)| template.replace("{id}", &node_id.to_string()))
}

/// Canonical work-item statuses (WI #285). Lifecycle: `open → resolved`
/// (implemented; may still need a user test / may not be PR'd) `→ done`
/// (agent satisfied — terminal but still visible in default lists)
/// `→ closed` (Ken only; hidden by default).
///
/// `parked` (WI #810, sprint 054) is off that line entirely — it is not a later
/// stage of `open` but a sideways move from it: work that is real and wanted but
/// waiting on a condition rather than on a person ("things to consider when a
/// condition is met"; #406 and #780 were the examples). It stays *visible*,
/// because the whole point is to keep it in view; it just sorts to the bottom
/// under a divider so it never competes with what is actionable now.
///
/// It carries no DB constraint to widen: `workitem.wi_status` is bare
/// `TEXT NOT NULL` (0001) and always has been, so korg-core is not merely the
/// authority here (WI #526) but the *only* enforcement. That is why adding this
/// value is a one-line vocabulary change and not a migration — and why the two
/// partition fences below are the only thing standing between a new status and
/// a silently mis-filed corpus.
pub const WI_STATUSES: [&str; 5] = ["open", "resolved", "done", "closed", "parked"];

/// The work items a list read means by default (WI #861), i.e. everything that
/// is not terminal. The program plan's §8 answer 1 decided the split: `closed`
/// is 78% of the corpus and is Ken's own "I am finished with this" marker;
/// `resolved` and `done` stay visible, because `done`'s visibility is already a
/// promise `update_work_item`'s schema makes ("terminal but visible in default
/// lists") and `resolved` is the may-still-want-to-see state.
/// `parked` is live (#810): a parked item is one Ken has explicitly asked to
/// keep in view, so hiding it by default would defeat the status. The divider
/// does the de-prioritising that hiding would otherwise do, and does it without
/// making the row unfindable.
pub const WI_LIVE_STATUSES: [&str; 4] = ["open", "resolved", "done", "parked"];

/// The complement of [`WI_LIVE_STATUSES`] — excluded from a lean
/// `list_work_items` unless asked for, and counted in its `omitted`. Kept beside
/// the full vocabulary so a fifth status cannot be added without deciding which
/// side it falls on; `wi_statuses_partition_cleanly` fences that.
pub const WI_TERMINAL_STATUSES: [&str; 1] = ["closed"];

/// The statuses that mean **the work is finished** (#978, sprint 053).
///
/// **This is not [`WI_TERMINAL_STATUSES`], and confusing the two is the whole
/// reason it has a name.** That split is about *list visibility* — which rows a
/// lean `list_work_items` hides — and it holds `closed` alone, because `done`
/// stays visible by design. This split is about *completion*: whether a thing
/// depending on this item is still waiting. `done` is "agent satisfied", which
/// this doc comment has called terminal since #285, so a dependency on a `done`
/// item is satisfied even though the row is still on screen.
///
/// A reader that used `WI_TERMINAL_STATUSES` here would report every `done`
/// dependency as an unmet blocker — a deterministic panel confidently wrong,
/// which is precisely what #977 and #978 exist to prevent.
///
/// This is also the set `advance_completed_anchor` has always used (sprint 051:
/// finishing a materialised maintenance item restarts a `completed`-anchored
/// schedule's clock). Naming it makes the two agree by construction rather than
/// by two matching literals in different files.
pub const WI_FINISHED_STATUSES: [&str; 2] = ["done", "closed"];

/// The complement of [`WI_FINISHED_STATUSES`] — a dependency in one of these
/// still blocks. `resolved` is the interesting member: "implemented; may still
/// need a user test / may not be PR'd" is not landed, and a queue row told it
/// was unblocked by unlanded work would be told to start on sand.
///
/// `parked` is unfinished and it is the *most* blocking value in the set: work
/// deferred until a condition fires is the definition of not-yet-done, and #978's
/// board derivation must report a dependency on one as an unmet blocker. Calling
/// it finished because it is "not being worked on" would tell a reader that a
/// thing waiting indefinitely had been dealt with.
pub const WI_UNFINISHED_STATUSES: [&str; 3] = ["open", "resolved", "parked"];

/// Work-item types (D-2). `brainstorm` is deliberate: half-formed ideas get
/// filed as work items rather than lost.
///
/// `maintenance` (sprint 051, #581) is the type a [`schedule`](SCHEDULE_CADENCES)
/// materialises into. The WI asked for "its own type" so kmon and other
/// automation can find generated items as a group — a vocabulary value rather
/// than a title convention, because a convention is not something a query can
/// filter on.
pub const WI_TYPES: [&str; 8] = [
    "task",
    "bug",
    "chore",
    "feature",
    "research",
    "tweak",
    "brainstorm",
    "maintenance",
];

/// T-shirt sizes; mirrors the `wi_tshirt` CHECK in migration 0001.
pub const WI_TSHIRTS: [&str; 7] = ["XS", "S", "M", "L", "XL", "Huge", "Unknown"];

/// Kanban columns; mirrors the `card_status` enum in migration 0001.
pub const CARD_STATUSES: [&str; 6] = ["Backlog", "Research", "OnDeck", "Active", "Done", "Cut"];

/// The columns that answer an awaiting-Ken ask (#1389, D-7).
///
/// D-7's rule is *a state only Ken sets clears the marker*, and the kanban is
/// entirely Ken's board — reaching the last column is him answering, and `Cut`
/// makes the ask moot the way archiving does. The other four columns are work
/// in motion and leave the ask standing.
///
/// A named constant rather than two literals in the clearing SQL because that
/// is exactly how `parked` got missed one vocabulary over: hand-written copies
/// of a status set do not move when the set does.
/// `card_terminal_statuses_are_card_statuses` fences the spelling.
pub const CARD_TERMINAL_STATUSES: [&str; 2] = ["Done", "Cut"];

/// Reading-list dispositions; mirrors the `link_disposition` enum (0004).
pub const LINK_DISPOSITIONS: [&str; 5] = ["Unread", "Done", "Revisit", "Summarized", "VaultSaved"];

/// Sprint-proposal lifecycle; mirrors `sprint_proposal_status` (0008).
pub const PROPOSAL_STATUSES: [&str; 4] = ["proposed", "active", "done", "declined"];

/// The proposals a queue read means by default (WI #852): the live ones. The
/// complement — `done` and `declined` — was 71% of `list_proposals`' payload and
/// nothing an agent picking a sprint has ever wanted. Kept beside the full
/// vocabulary so a fifth status cannot be added without deciding which side it
/// falls on; `proposal_statuses_partition_cleanly` fences that.
pub const PROPOSAL_LIVE_STATUSES: [&str; 2] = ["proposed", "active"];

/// The complement of [`PROPOSAL_LIVE_STATUSES`] — excluded from a lean
/// `list_proposals` unless asked for, and counted in its `omitted`.
pub const PROPOSAL_TERMINAL_STATUSES: [&str; 2] = ["done", "declined"];

/// The proposals somebody has actually **begun** (#1424, sprint 069) — a
/// different cut from live/terminal, and the one a program's `queued` state is
/// defined against.
///
/// Not the complement of `proposed`, which is the tempting shortcut: `declined`
/// is a decision *not* to do the work, so a program whose only slice was
/// dropped was never started. `done` is here because work that shipped was
/// plainly begun, whether or not anyone remembered to mark it `active` on the
/// way through — a program cannot be waiting to start on a slice that has
/// already finished.
///
/// One name because three places ask the same question and must not diverge:
/// `create_program`'s initial status, the promotion out of `queued`, and 0030's
/// backfill.
pub const PROPOSAL_STARTED_STATUSES: [&str; 2] = ["active", "done"];

/// Program lifecycle (#968, sprint 044; `queued` added #1424, sprint 069);
/// mirrors the `program.status` CHECK (0023, widened by 0030). A program is the
/// cross-project layer: it `includes` proposals, ordered, and outlives any one
/// of them. Ordered as the lifecycle reads: `queued` → `active` ⇄ `holding` →
/// `done`.
///
/// `holding` is the state a program spends most of its life in and the reason
/// this is not just `active`/`done` — a program whose current slice has shipped
/// and whose next one is not started is neither finished nor in flight, and
/// collapsing that into `active` would make the Operations panel claim work is
/// underway when nobody is on it.
///
/// **`queued` is that same argument one step earlier**, and it exists because
/// the panel made exactly that claim: 0023 defaulted the column to `active` and
/// `create_program` never wrote it, so a program that had been planned and not
/// begun rendered with an ACTIVE callout (#1424, with the screenshot). It means
/// precisely *no slice of this program has started* — not "ready", which would
/// assert a readiness korg cannot check, and not "created", which would name how
/// the row got here rather than where the work stands.
///
/// Read it as a fact about the slices, because that is what korg maintains it
/// as: every path that can start a slice — `update_proposal`, `create_program`,
/// `relate` — promotes a `queued` program to `active`, so the state can never
/// outlive the condition it asserts. It does **not** come back: a program that
/// has begun and paused is `holding`, which is somebody's decision, and korg
/// never demotes into `queued` behind them.
///
/// This prose is load-bearing rather than decorative. kfdc #1196 exists because
/// a status value's documentation asserted a meaning korg did not have, and a
/// plausible-sounding comment kept the resulting bug alive; kfdc switches its
/// Operations colour on these literals.
pub const PROGRAM_STATUSES: [&str; 4] = ["queued", "active", "holding", "done"];

/// The state a program is born in — written explicitly by `create_program`
/// (#526: the vocabulary is the authority, the DB default is the backstop) and
/// mirrored by 0030's column default so a direct INSERT cannot disagree.
///
/// A named constant rather than a literal at the insert site because that is
/// how the two drifted in the first place: 0023 set a default and the code that
/// should have owned the choice simply omitted the column.
pub const PROGRAM_INITIAL_STATUS: &str = "queued";

/// The programs a list read means by default — the ones still going. Same split
/// as proposals and work items, and fenced by the same partition test so a new
/// status cannot be added without deciding which side it falls on.
///
/// `queued` is **live**: a program nobody has started yet is the thing the
/// Operations panel most needs to show, and filing it terminal would hide it
/// from the read that exists to display it.
pub const PROGRAM_LIVE_STATUSES: [&str; 3] = ["queued", "active", "holding"];

/// The complement of [`PROGRAM_LIVE_STATUSES`] — excluded from a default
/// `list_programs` and counted in its `omitted`.
pub const PROGRAM_TERMINAL_STATUSES: [&str; 1] = ["done"];

/// Daily-report statuses; mirrors the `report.status` CHECK (0010).
pub const REPORT_STATUSES: [&str; 3] = ["ok", "attention", "problem"];

// --- time-derived surfacing (sprint 051: #581 + #950) -----------------------

/// How often a [`schedule`] comes round again; mirrors the `schedule.cadence`
/// CHECK (0025, widened by 0028). Ordered by interval, shortest first.
///
/// `once` is not a special case with its own storage — it is the cadence whose
/// interval is **zero**, so `schedule.anchor_at` simply *is* the fire date.
/// That is what let the one-shot (Ken's DST-recheck example) and the quarterly
/// restore drill share one node shape instead of two, which korg:1079 argued
/// was the real cost of the slice that was rejected.
///
/// **The set mixes two families, and that is what #1113 found.** `weekly` and
/// `fortnightly` are whole weeks, so they preserve a *weekday*; `monthly`,
/// `quarterly` and `yearly` advance by calendar month and drift off it
/// (2026-08-08 is a Saturday, 2026-09-08 is a Tuesday). The original four were
/// written before anyone had filed a real schedule, and on first contact one of
/// the two recurring requests — "every two weeks, Saturday noon" — fell outside
/// them, expressible only as `weekly` with a note saying it was an interim.
/// `fortnightly` (sprint 063) is the missing member of the week family, added
/// by the same move `once` established: a cadence is just an interval.
///
/// Deliberately **not** generalised to `every_n_weeks: 2`. Named values keep
/// the vocabulary closed and greppable — the property `validate_status`, the
/// `schedule_cadence_check` constraint and the docs_drift vocabulary fences all
/// rest on. The question is recorded here rather than re-derived: revisit it
/// when a *third* week-based interval is actually asked for, not before.
pub const SCHEDULE_CADENCES: [&str; 6] = [
    "once",
    "weekly",
    "fortnightly",
    "monthly",
    "quarterly",
    "yearly",
];

/// Which event advances `schedule.anchor_at` — #581's "two styles", named.
///
/// * `completed` — duration from the last time the maintenance was *done*.
///   Advanced by a korg-core write rule when the materialised work item reaches
///   a completed status, never by a trigger (0023's precedent for the awaiting
///   marker: lifecycle rules live in the one path both transports share).
/// * `created` — duration from the last time the work item was *created*.
///   Advanced by `materialize_schedule` itself.
///
/// Both are one column. The styles differ only in which event moves it, which
/// is why the second one was not the seam korg:1079 said to cut if scope had to
/// give.
pub const SCHEDULE_ANCHORS: [&str; 2] = ["completed", "created"];

/// Schedule lifecycle; mirrors the `schedule.status` CHECK (0025).
///
/// `paused` is the schedule-side analogue of a retired report source: a drill
/// that should stop surfacing for a while without losing its history or its
/// anchor. `done` is terminal and is where a `once` schedule lands the moment it
/// materialises.
pub const SCHEDULE_STATUSES: [&str; 3] = ["active", "paused", "done"];

/// The schedules a list read means by default — same split as proposals,
/// programs and work items, fenced by the same partition test.
pub const SCHEDULE_LIVE_STATUSES: [&str; 2] = ["active", "paused"];

/// The complement of [`SCHEDULE_LIVE_STATUSES`] — excluded from a default
/// `list_schedules` and counted in its `omitted`.
pub const SCHEDULE_TERMINAL_STATUSES: [&str; 1] = ["done"];

/// The closed set of template substitutions (#581, D-6).
///
/// Ken was explicit that `"korg restore drill - {MONTH} {YEAR} (Quarterly)"`
/// conveys the *need* and is not the spec, so this stays a small closed set
/// rather than growing a template syntax. An unrecognised placeholder is
/// rejected at write time with this list in the message: the alternative is a
/// typo rendering literally into a work-item title months later, which is the
/// exact class of silent wrong this sprint exists to remove.
pub const SCHEDULE_SUBSTITUTIONS: [&str; 5] = ["YEAR", "MONTH", "DAY", "DATE", "QUARTER"];

/// Whether korg can currently believe what a report source last said (#950).
///
/// * `fresh` — filed within its cadence plus grace.
/// * `stale` — overdue. **The alert.** A source that filed daily and stopped is
///   itself the signal, because absence of a write is precisely what a broken
///   tool can never report about itself.
/// * `retired` — declared ended (`report_source.retired`). Deliberately stopped,
///   so not an alert. An explicit declaration is the only thing that keeps "we
///   turned this off" distinguishable from "this died"; silence cannot.
/// * `unrated` — too little history to infer a cadence and none declared.
///
/// `unrated` exists because both available guesses are bad. Calling an unknown
/// source `fresh` rebuilds the July 2026 failure this WI is about; calling it
/// `stale` cries wolf on every one-off report and trains people to ignore the
/// panel — and a channel nobody looks at is not a channel. Declaring a cadence
/// is how a real source leaves this state on its first day.
pub const SOURCE_FRESHNESS: [&str; 4] = ["fresh", "stale", "retired", "unrated"];

/// What a report source currently asserts about its subject — [`REPORT_STATUSES`]
/// plus `unknown`.
///
/// **The rule #950 exists for**: anything but `fresh` asserts `unknown`, never
/// the last known status. In July 2026 korg served a GREEN from 2026-07-22 for
/// eleven days *because* the thing checking the fleet was broken; restating a
/// last-known status in a stale card reproduces that failure exactly.
/// `source_asserts_extends_report_statuses` keeps the two lists from drifting.
pub const SOURCE_ASSERTIONS: [&str; 4] = ["ok", "attention", "problem", "unknown"];

/// What a source asserts when korg cannot vouch for it — the value that must
/// appear on every non-`fresh` row.
pub const SOURCE_ASSERTS_UNKNOWN: &str = SOURCE_ASSERTIONS[3];

/// Project lifecycle statuses (WI #246, narrowed to two by #828). Default
/// WI-page rail shows only `active` unless "show all" is on.
///
/// `maintenance` and `inactive` were dropped in 0020 on the same argument: both
/// held **zero rows** and neither had semantics distinct from `archived`, which
/// makes an unused vocabulary value a standing invitation for a future writer to
/// invent a meaning nobody agreed. The live corpus was already 27 `active` /
/// 8 `archived` when they were removed.
///
/// The status answers one question — *is this row a legitimate target for new
/// work?* — and that question has two answers. Where `archived` covers two
/// genuinely different situations (superseded: `kwi`/`kcard` → korg; dormant:
/// `trt-llm-explore`), the difference lives in the `description` prose, because
/// both filter identically and a status exists to filter.
pub const PROJECT_STATUSES: [&str; 2] = ["active", "archived"];

/// The one status that is a legitimate target for new work — the question
/// above, named, so the write paths that enforce it (`resolve_project`,
/// `resolve_project_patch`; WI #884) read as the sentence rather than as a bare
/// string comparison, and so a third status could not be added without this
/// line forcing the "which side?" decision.
pub const PROJECT_STATUS_ACTIVE: &str = PROJECT_STATUSES[0];

/// Project categories (WI #678). `project.category` has existed since 0011 but
/// as free text, and it drifted the way free text does — the live corpus held
/// `ai` x8, `AI` x1, `tooling` x3, `infra` x2, `fun` x2 and NULL x15. This
/// closes the set the way LB-2 closed the relationship labels, and the Work
/// Items rail colors from it instead of hashing the project *name* (which put
/// 20 of 31 projects within 12 degrees of another while leaving whole arcs of
/// the wheel empty).
///
/// Adding a category is one edit here plus its hue in `CATEGORY_HUE` in
/// `web/src/lib/domain.ts`, then `just gen`. `EVAL` is the worked example:
/// added 2026-07-26 for harness-evaluation projects that leaked into the real
/// corpus, so that they are findable as a group.
pub const PROJECT_CATEGORIES: [&str; 7] = [
    "AI",
    "Dashboard",
    "EVAL",
    "Fun",
    "Infrastructure",
    "Ops",
    "Other",
];

/// Every vocabulary, as `(const name, TypeScript type name, values)`. Adding a
/// set here is all it takes for the web app to see it: `just gen` writes them
/// into `web/src/lib/generated/vocab.ts`, so the UI's status lists stop being a
/// hand-kept copy that drifts (the old `api.ts` `WI_TYPES` had nine entries,
/// six of which the server rejects). `docs_drift` (korg-mcp) walks it too, so a
/// vocabulary enumerated in prose is checked against this registry, not a copy.
pub const EXPORTED: [(&str, &str, &[&str]); 18] = [
    ("NODE_KINDS", "NodeKind", &NODE_KINDS),
    ("WI_STATUSES", "WiStatus", &WI_STATUSES),
    ("WI_TYPES", "WiType", &WI_TYPES),
    ("WI_TSHIRTS", "WiTshirt", &WI_TSHIRTS),
    ("CARD_STATUSES", "CardStatus", &CARD_STATUSES),
    ("LINK_DISPOSITIONS", "Disposition", &LINK_DISPOSITIONS),
    ("PROPOSAL_STATUSES", "ProposalStatus", &PROPOSAL_STATUSES),
    ("PROGRAM_STATUSES", "ProgramStatus", &PROGRAM_STATUSES),
    ("REPORT_STATUSES", "ReportStatus", &REPORT_STATUSES),
    ("PROJECT_STATUSES", "ProjectStatus", &PROJECT_STATUSES),
    ("PROJECT_CATEGORIES", "ProjectCategory", &PROJECT_CATEGORIES),
    ("SCHEDULE_CADENCES", "ScheduleCadence", &SCHEDULE_CADENCES),
    ("SCHEDULE_ANCHORS", "ScheduleAnchor", &SCHEDULE_ANCHORS),
    ("SCHEDULE_STATUSES", "ScheduleStatus", &SCHEDULE_STATUSES),
    (
        "SCHEDULE_SUBSTITUTIONS",
        "ScheduleSubstitution",
        &SCHEDULE_SUBSTITUTIONS,
    ),
    ("SOURCE_FRESHNESS", "SourceFreshness", &SOURCE_FRESHNESS),
    ("SOURCE_ASSERTIONS", "SourceAssertion", &SOURCE_ASSERTIONS),
    // Not a domain vocabulary but the same kind of fact: a closed set the
    // client must agree with. The web app branches on it to tell "you typed
    // something wrong" from "korg broke" (sprint 019).
    ("ERROR_CODES", "ErrorCode", &crate::error::ERROR_CODES),
];

/// Every **named subset** of an [`EXPORTED`] vocabulary — the partitions that
/// prose legitimately enumerates instead of the whole set (#1387).
///
/// The docs fence reads a pipe-span as a claim about a *whole* vocabulary. Tool
/// descriptions are not like that: they say "everything not terminal
/// (`open`, `resolved`, `done`, `parked`)" and "a proposal reaching
/// `done`/`declined`", which are true statements about a partition. Without a
/// registry of the legal partitions, a fence over that prose can only choose
/// between rejecting correct sentences and accepting stale ones — F-4 is
/// precisely a stale one (`open`/`resolved` for what is now three statuses).
/// So the partitions get names here, and a description may enumerate any set
/// in this table or in `EXPORTED`, and nothing else.
pub const PARTITIONS: [(&str, &[&str]); 12] = [
    ("WI_LIVE_STATUSES", &WI_LIVE_STATUSES),
    ("WI_TERMINAL_STATUSES", &WI_TERMINAL_STATUSES),
    ("WI_FINISHED_STATUSES", &WI_FINISHED_STATUSES),
    ("WI_UNFINISHED_STATUSES", &WI_UNFINISHED_STATUSES),
    ("PROPOSAL_LIVE_STATUSES", &PROPOSAL_LIVE_STATUSES),
    ("PROPOSAL_TERMINAL_STATUSES", &PROPOSAL_TERMINAL_STATUSES),
    ("PROPOSAL_STARTED_STATUSES", &PROPOSAL_STARTED_STATUSES),
    ("PROGRAM_LIVE_STATUSES", &PROGRAM_LIVE_STATUSES),
    ("PROGRAM_TERMINAL_STATUSES", &PROGRAM_TERMINAL_STATUSES),
    ("SCHEDULE_LIVE_STATUSES", &SCHEDULE_LIVE_STATUSES),
    ("SCHEDULE_TERMINAL_STATUSES", &SCHEDULE_TERMINAL_STATUSES),
    ("CARD_TERMINAL_STATUSES", &CARD_TERMINAL_STATUSES),
];

/// Reject a value outside its vocabulary with the full allowed set in the
/// message — the error doubles as the documentation an agent needs to retry.
pub fn validate(value: &str, allowed: &[&str], what: &str) -> Result<(), RepoError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(RepoError::InvalidInput(format!(
            "invalid {what} '{value}' — expected one of: {}",
            allowed.join(", ")
        )))
    }
}

/// Emit the vocabularies — and the relationship-label registry — as
/// TypeScript, next to the `ts-rs` output (WI #541).
///
/// This runs as a test because that is where `ts-rs` generates too, so one
/// `just gen` invocation produces the whole `web/src/lib/generated/` directory
/// and CI's `git diff --exit-code` covers all of it. Naming it
/// `export_bindings_*` puts it in the same `cargo test export_bindings` filter
/// `ts-rs` uses.
#[cfg(test)]
mod partition {
    use super::{
        node_path, CARD_STATUSES, CARD_TERMINAL_STATUSES, NODE_KINDS, NODE_ROUTES,
        PROGRAM_LIVE_STATUSES, PROGRAM_STATUSES, PROGRAM_TERMINAL_STATUSES, PROPOSAL_LIVE_STATUSES,
        PROPOSAL_STATUSES, PROPOSAL_TERMINAL_STATUSES, REPORT_STATUSES, SCHEDULE_LIVE_STATUSES,
        SCHEDULE_STATUSES, SCHEDULE_TERMINAL_STATUSES, SOURCE_ASSERTIONS, SOURCE_ASSERTS_UNKNOWN,
        SOURCE_FRESHNESS, WI_FINISHED_STATUSES, WI_LIVE_STATUSES, WI_STATUSES,
        WI_TERMINAL_STATUSES, WI_UNFINISHED_STATUSES,
    };
    use std::collections::BTreeSet;

    /// A lean `list_proposals` shows the live statuses and counts the terminal
    /// ones in `omitted` (WI #852). A fifth proposal status added to the enum
    /// and to `PROPOSAL_STATUSES` but to neither half would be silently
    /// invisible *and* uncounted — the exact failure `omitted` exists to stop.
    #[test]
    fn proposal_statuses_partition_cleanly() {
        let all: BTreeSet<&str> = PROPOSAL_STATUSES.into_iter().collect();
        let live: BTreeSet<&str> = PROPOSAL_LIVE_STATUSES.into_iter().collect();
        let terminal: BTreeSet<&str> = PROPOSAL_TERMINAL_STATUSES.into_iter().collect();
        assert!(
            live.is_disjoint(&terminal),
            "a proposal status is both live and terminal: {:?}",
            live.intersection(&terminal).collect::<Vec<_>>()
        );
        assert_eq!(
            all,
            live.union(&terminal).copied().collect::<BTreeSet<&str>>(),
            "every proposal status must be classified live or terminal — \
             list_proposals' default and its `omitted` counts both depend on it"
        );
    }

    /// The same fence for work items (WI #861), and it matters more here: a
    /// fifth work-item status in neither half would drop out of the default
    /// `list_work_items` **and** out of `omitted.closed`, which is precisely the
    /// silent under-count the envelope exists to prevent.
    #[test]
    fn wi_statuses_partition_cleanly() {
        let all: BTreeSet<&str> = WI_STATUSES.into_iter().collect();
        let live: BTreeSet<&str> = WI_LIVE_STATUSES.into_iter().collect();
        let terminal: BTreeSet<&str> = WI_TERMINAL_STATUSES.into_iter().collect();
        assert!(
            live.is_disjoint(&terminal),
            "a work-item status is both live and terminal: {:?}",
            live.intersection(&terminal).collect::<Vec<_>>()
        );
        assert_eq!(
            all,
            live.union(&terminal).copied().collect::<BTreeSet<&str>>(),
            "every work-item status must be classified live or terminal — \
             list_work_items' default and its `omitted` counts both depend on it"
        );
    }

    /// The *completion* split (#978), fenced separately from the visibility one
    /// above because they are different questions over the same vocabulary and
    /// a fifth status has to be classified on both axes.
    ///
    /// The assertion that matters is the last one: these two partitions must
    /// **not** be the same set. If a future edit made them agree, the name
    /// `WI_FINISHED_STATUSES` would stop earning its existence and the next
    /// reader would collapse them back into one — reintroducing exactly the bug
    /// the split was created to prevent, in which every `done` dependency reads
    /// as an unmet blocker.
    #[test]
    fn wi_statuses_partition_by_completion_too() {
        let all: BTreeSet<&str> = WI_STATUSES.into_iter().collect();
        let finished: BTreeSet<&str> = WI_FINISHED_STATUSES.into_iter().collect();
        let unfinished: BTreeSet<&str> = WI_UNFINISHED_STATUSES.into_iter().collect();
        assert!(
            finished.is_disjoint(&unfinished),
            "a work-item status is both finished and unfinished: {:?}",
            finished.intersection(&unfinished).collect::<Vec<_>>()
        );
        assert_eq!(
            all,
            finished
                .union(&unfinished)
                .copied()
                .collect::<BTreeSet<&str>>(),
            "every work-item status must be classified finished or unfinished — \
             the board's blocker derivation (#978) asks this of every dependency"
        );
        let terminal: BTreeSet<&str> = WI_TERMINAL_STATUSES.into_iter().collect();
        assert_ne!(
            finished, terminal,
            "WI_FINISHED_STATUSES and WI_TERMINAL_STATUSES must stay different \
             sets — `done` is finished work that a lean list still shows, and \
             collapsing the two makes every `done` dependency read as a blocker"
        );
    }

    /// `parked` sits on the unusual corner of both partitions (#810), and the
    /// combination is the whole status: **visible but unfinished**. Every other
    /// value is either finished-and-visible (`done`), finished-and-hidden
    /// (`closed`) or unfinished-and-visible-because-it-is-active
    /// (`open`/`resolved`). Parked is unfinished and visible *on purpose*.
    ///
    /// Asserted directly rather than left to the two partition tests above,
    /// which only check that every status lands on some side — they would pass
    /// just as happily with `parked` filed as terminal (making it invisible,
    /// defeating the status) or as finished (making a dependency on indefinitely
    /// deferred work read as satisfied). Those are the two plausible mistakes,
    /// so they get a test that names them.
    #[test]
    fn parked_is_visible_but_never_counts_as_finished() {
        assert!(
            WI_LIVE_STATUSES.contains(&"parked"),
            "`parked` must stay in default listings — it exists to keep work in \
             view, and hiding it is what `closed` is for"
        );
        assert!(
            !WI_TERMINAL_STATUSES.contains(&"parked"),
            "`parked` must not be terminal"
        );
        assert!(
            WI_UNFINISHED_STATUSES.contains(&"parked"),
            "`parked` work is not done — #978's board must report a dependency \
             on a parked item as an unmet blocker"
        );
        assert!(
            !WI_FINISHED_STATUSES.contains(&"parked"),
            "`parked` must never satisfy a dependency"
        );
    }

    /// Every node kind has a page to link to (WI #1467). The fence that makes
    /// [`NODE_ROUTES`] a contract rather than a list somebody remembers to
    /// extend: adding a tenth kind to [`NODE_KINDS`] fails here until it has a
    /// route, which is the whole reason korg can promise consumers that a URL
    /// exists for anything it hands them.
    ///
    /// `schedule` is why this is a test and not a comment. Sprint 051 added it
    /// to the `node.kind` constraint and to nothing else; `get_node_preview`
    /// went three sprints without an arm for it, guarded by a fence that
    /// iterated its own hardcoded list of seven. This one iterates
    /// [`NODE_KINDS`], so it cannot be told a smaller truth.
    #[test]
    fn every_node_kind_has_a_route() {
        for kind in NODE_KINDS {
            let path = node_path(kind, 42).unwrap_or_else(|| {
                panic!(
                    "node kind {kind:?} has no entry in NODE_ROUTES — every kind \
                     korg holds must be reachable at a URL, or the consumers \
                     that route by locator have to guess"
                )
            });
            assert!(
                path.starts_with('/') && !path.contains("{id}"),
                "{kind}'s route {path:?} is not a resolved absolute path"
            );
            assert!(
                path.ends_with("/42"),
                "{kind}'s route {path:?} must end in the node id — `/n/:node_id` \
                 redirects here and the id is the only thing it knows"
            );
        }
        assert_eq!(
            NODE_ROUTES.len(),
            NODE_KINDS.len(),
            "NODE_ROUTES has a row for a kind that is not in NODE_KINDS"
        );
    }

    /// Two kinds sharing a path would make `/n/:node_id` ambiguous to read back
    /// — a URL would no longer say what it points at, which is the property
    /// per-kind routes (#621) were chosen for in the first place.
    #[test]
    fn node_routes_are_distinct() {
        let paths: BTreeSet<&str> = NODE_ROUTES.iter().map(|(_, p)| *p).collect();
        assert_eq!(
            paths.len(),
            NODE_ROUTES.len(),
            "two node kinds share a route template"
        );
    }

    /// A kind korg does not have gets `None`, not a fabricated path. GP-13's
    /// consumer half in the one place korg is itself the consumer: `kind`
    /// arrives as a `String` off the wire, and the honest answer to an
    /// unrecognised one is "I cannot say".
    #[test]
    fn an_unknown_kind_has_no_route() {
        assert_eq!(node_path("topic", 42), None);
    }

    /// Every entry in [`PARTITIONS`] must actually be a subset of some
    /// [`EXPORTED`] vocabulary, or the tool-description fence it feeds would
    /// bless prose enumerating values korg does not have.
    #[test]
    fn every_partition_is_a_subset_of_a_vocabulary() {
        for (name, values) in super::PARTITIONS {
            let set: BTreeSet<&str> = values.iter().copied().collect();
            assert!(
                super::EXPORTED.iter().any(|(_, _, all)| {
                    set.is_subset(&all.iter().copied().collect::<BTreeSet<&str>>())
                }),
                "{name} is not a subset of any vocabulary in EXPORTED"
            );
            assert!(!set.is_empty(), "{name} is empty");
        }
    }

    /// `CARD_TERMINAL_STATUSES` is a subset of the columns, not a parallel
    /// vocabulary (#1389). It feeds SQL comparisons against `card.status`, so a
    /// typo or a renamed column would not fail — it would silently stop
    /// clearing, leaving answered asks in the Commander's Call lane, which is
    /// the one failure the marker exists to prevent.
    #[test]
    fn card_terminal_statuses_are_card_statuses() {
        let all: BTreeSet<&str> = CARD_STATUSES.into_iter().collect();
        let terminal: BTreeSet<&str> = CARD_TERMINAL_STATUSES.into_iter().collect();
        assert!(
            terminal.is_subset(&all),
            "not a kanban column: {:?}",
            terminal.difference(&all).collect::<Vec<_>>()
        );
        assert!(
            !terminal.is_empty() && terminal.len() < all.len(),
            "the terminal columns are some of the columns — all of them would \
             clear every ask the moment a card moved"
        );
    }

    /// And for programs (#968, sprint 044). `holding` is the value this fence
    /// exists for: it is neither in flight nor finished, and a fourth status
    /// with the same ambiguity must not be able to slip in unclassified.
    #[test]
    fn program_statuses_partition_cleanly() {
        let all: BTreeSet<&str> = PROGRAM_STATUSES.into_iter().collect();
        let live: BTreeSet<&str> = PROGRAM_LIVE_STATUSES.into_iter().collect();
        let terminal: BTreeSet<&str> = PROGRAM_TERMINAL_STATUSES.into_iter().collect();
        assert!(
            live.is_disjoint(&terminal),
            "a program status is both live and terminal: {:?}",
            live.intersection(&terminal).collect::<Vec<_>>()
        );
        assert_eq!(
            all,
            live.union(&terminal).copied().collect::<BTreeSet<&str>>(),
            "every program status must be classified live or terminal — \
             list_programs' default and its `omitted` counts both depend on it"
        );
    }

    /// And for schedules (sprint 051, #581). `paused` is the value this fence
    /// exists for — it is neither running nor finished, exactly the ambiguity
    /// `holding` has for programs, and a fourth status sharing it must not be
    /// able to slip in unclassified.
    #[test]
    fn schedule_statuses_partition_cleanly() {
        let all: BTreeSet<&str> = SCHEDULE_STATUSES.into_iter().collect();
        let live: BTreeSet<&str> = SCHEDULE_LIVE_STATUSES.into_iter().collect();
        let terminal: BTreeSet<&str> = SCHEDULE_TERMINAL_STATUSES.into_iter().collect();
        assert!(
            live.is_disjoint(&terminal),
            "a schedule status is both live and terminal: {:?}",
            live.intersection(&terminal).collect::<Vec<_>>()
        );
        assert_eq!(
            all,
            live.union(&terminal).copied().collect::<BTreeSet<&str>>(),
            "every schedule status must be classified live or terminal — \
             list_schedules' default and its `omitted` counts both depend on it"
        );
    }

    /// A source asserts a report status, or it asserts `unknown` — nothing
    /// else. A sixth report status added without touching [`SOURCE_ASSERTIONS`]
    /// would leave a fresh source asserting a value outside its own declared
    /// vocabulary, which is the sort of drift a generated TypeScript union
    /// turns into a compile error in the web app and a silent lie in an agent.
    #[test]
    fn source_asserts_extends_report_statuses() {
        let asserts: BTreeSet<&str> = SOURCE_ASSERTIONS.into_iter().collect();
        let expected: BTreeSet<&str> = REPORT_STATUSES
            .into_iter()
            .chain([SOURCE_ASSERTS_UNKNOWN])
            .collect();
        assert_eq!(
            asserts, expected,
            "SOURCE_ASSERTIONS must be exactly REPORT_STATUSES + '{SOURCE_ASSERTS_UNKNOWN}'"
        );
        assert!(
            !REPORT_STATUSES.contains(&SOURCE_ASSERTS_UNKNOWN),
            "'{SOURCE_ASSERTS_UNKNOWN}' must not also be a report status — the whole \
             point is that it is the value korg substitutes when it cannot vouch \
             for what a source last said (#950)"
        );
    }

    /// Exactly one freshness value is the alert (#950). If a future edit made
    /// `retired` or `unrated` alert too, the panel starts crying wolf and a
    /// channel nobody looks at is not a channel — which is the failure mode the
    /// WI names directly.
    #[test]
    fn exactly_one_freshness_is_the_alert() {
        assert!(SOURCE_FRESHNESS.contains(&"stale"));
        assert_eq!(
            SOURCE_FRESHNESS.len(),
            4,
            "adding a freshness value means deciding whether it alerts — see \
             `SourceHealth::alerts` in repo/reports.rs, which this list feeds"
        );
    }
}

#[cfg(test)]
mod generate {
    use super::{EXPORTED, NODE_ROUTES};
    use crate::relationships::REGISTRY;

    fn quoted(values: &[&str]) -> String {
        values
            .iter()
            .map(|v| format!("\"{v}\""))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn render() -> String {
        let mut out = String::from(
            "// This file is generated by `just gen` from korg-core. Do not edit this file manually.\n",
        );
        for (name, ty, values) in EXPORTED {
            out.push_str(&format!(
                "\nexport const {name} = [{}] as const;\nexport type {ty} = (typeof {name})[number];\n",
                quoted(values)
            ));
        }
        out.push_str(
            "\n/** The complete, closed set of relationship labels korg accepts (LB-2):\n \
             *  relate() rejects anything else. Extend it by adding a registry entry in\n \
             *  korg-core and running `just gen`. */\nexport const \
             RELATIONSHIP_LABELS = [\n",
        );
        for spec in REGISTRY {
            out.push_str(&format!(
                "  {{ label: \"{}\", directed: {}, reads: \"{}\" }},\n",
                spec.label, spec.directed, spec.reads
            ));
        }
        out.push_str("] as const;\nexport type RelationshipLabel = (typeof RELATIONSHIP_LABELS)[number][\"label\"];\n");

        // The route table (#1467). Generated rather than hand-kept in
        // domain.ts so korg's own UI reads the same authority `/n/:node_id`
        // and `NodePreview.url` are built from — a second copy in the web app
        // is the drift this table exists to end, even though the web app is
        // korg rather than a consumer of it.
        out.push_str(
            "\n/** Where each node kind is rendered. korg-core owns this table; `/n/:node_id`\n \
             *  and the `url` on a node ref are built from the same rows, so a consumer\n \
             *  never has to reconstruct it. */\nexport const NODE_ROUTES = {\n",
        );
        for (kind, template) in NODE_ROUTES {
            out.push_str(&format!("  {kind}: \"{template}\",\n"));
        }
        out.push_str("} as const;\n");
        out
    }

    #[test]
    fn export_bindings_vocab() {
        let dir = std::env::var("TS_RS_EXPORT_DIR").unwrap_or_else(|_| "./bindings".into());
        std::fs::create_dir_all(&dir).expect("create export dir");
        std::fs::write(std::path::Path::new(&dir).join("vocab.ts"), render())
            .expect("write vocab.ts");
    }
}
