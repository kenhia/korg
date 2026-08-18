//! korg-mcp library surface (so the tool dispatch is unit-testable).
pub mod tools;

/// The `instructions` an MCP client receives from `initialize` — the only
/// overview of korg an agent gets before it has called anything.
///
/// It therefore has to name every category of tool. Two sprints' worth of
/// clients were told korg covers work items, cards, links and relationships,
/// and never learned that sprint proposals, reports,
/// projects or comments existed (F-12). `docs_drift::the_server_instructions_
/// name_every_category` now fails if a category goes unmentioned; the full
/// catalogue is `docs/api.md`.
pub fn server_instructions() -> &'static str {
    "korg MCP server — one typed-node data model over Postgres covering work items, cards, \
     comments, reading-list links, generalized relationships, \
     sprint proposals, programs, reports, handoffs, schedules, attachments, projects and areas, \
     and a one-call \
     Board rollup. \
     Mutations validate their target and return the updated entity; errors are isError \
     results carrying {message, code} where code is one of invalid_input, not_found, \
     conflict, internal. Paginated collection reads (list_work_items, list_cards, \
     list_links, search) return {items, total, limit, offset}, where `total` is the \
     whole filtered corpus on every page — including one whose offset overshot the last \
     row, which returns no items and the same total; \
     the unpaginated ones (list_reports, list_areas, \
     list_comments, list_awaiting, list_report_sources, list_attachments) return a bare array and \
     have no archived \
     filter; and the filtered ones \
     (list_proposals, list_programs, list_projects, list_schedules) return {items, omitted}, where `omitted` counts \
     the rows their defaults hid — so a narrowed view can never be mistaken for the \
     whole corpus. list_work_items carries `omitted` too, for the \
     same reason: it is lean by default (no content/details) and hides `closed` \
     items unless you pass wi_status \"closed\" or \"all\". \
     Every read that filters archived rows excludes them by default: pass \
     `archived: null` for both, `true` for archived only. Writes take a project or area by name \
     (`project`/`area`) or by id (`project_id`/`area_id`), never both — except a program, \
     which takes NO project: it is the cross-project layer, and its span is derived from \
     the proposals it includes. \
     A `has_handoff` edge in a focused read (get_work_item/get_proposal/get_program) is \
     required context: get_handoff and read it before acting on the work — it \
     carries durable state another session left for you, not optional related \
     reading. \
     When something can only move once Ken acts — a decision, an ops action you cannot \
     perform, a review — mark it Awaiting Ken with set_awaiting rather than burying it \
     in a comment, and clear it yourself once you have the answer; list_awaiting is the \
     whole lane in one call. \
     Work that a DATE makes appear is a schedule, not a work item filed early: \
     recurring maintenance and one-shot \"look at this again after X\" both go to \
     create_schedule, and list_schedules(due_only:true) is what is due now. korg runs \
     no scheduler — due-ness is computed on read and materialize_schedule is an \
     explicit write. \
     Images are attachments: bytes go over REST (`curl -F file=@shot.png \
     '<korg>/api/img?owner=<node_id>'` to upload, `DELETE /api/img/<img-id>` to discard) and \
     metadata over MCP. get_work_item inlines a work item's attachments with a `url` per size; \
     to READ one, fetch its `agent` variant (≤1568px — bigger is bytes the model never sees) to \
     a temp file and read that file. An attachment with no has_attachment edge is `pending` and \
     is deleted 24 hours after upload, so an image you want kept must be owned. \
     A report source that filed on a cadence and then stopped is itself an alert: \
     list_report_sources says whether korg can still believe each one, and anything \
     not `fresh` asserts `unknown` rather than its last known status. \
     Search is a full-text read over the WHOLE corpus — every node kind plus every \
     COMMENT, which is where resolutions, verdicts and recon notes live. Reach for it \
     instead of paging a list and reading rows yourself when you are looking for \
     something rather than surveying: it is measurably better than a title scan, and it \
     is the only read that can see a comment. It is lexical, so phrase the query in the \
     words the record would use. Two of its response fields decide how to read the rest: \
     `relaxed` true means the all-terms query found nothing and this is the any-term \
     fallback (a broad answer, with a `total` to match), and by default it searches only \
     LIVE rows — pass scope:\"all\" whenever you are chasing something already decided \
     and closed, which is most historical questions. \
     When you want the WHOLE state of the work — what is being worked, what is queued, \
     what spans repos, what is waiting on Ken — call get_board once instead of walking \
     the queue proposal by proposal; it takes no arguments and is the read that exists \
     so nobody crawls."
}

/// [`server_instructions`] plus the live project roster (WI #674).
///
/// The roster exists to remove the *reason* for a reflexive `list_projects`
/// call, which #673 removed the *nudge* for. Neither works alone: telling an
/// agent "pass a name you are confident in" without supplying the names invites
/// a confident wrong guess, which is worse than the lookup.
///
/// Rendered at `initialize` rather than at process start, and that is the whole
/// design. korg is a long-running HTTP MCP server that can go weeks without a
/// restart during periods when korg itself is not being developed, so anything
/// baked at startup goes arbitrarily stale — and a refresh job would never
/// reach the running process. MCP returns `instructions` in the `initialize`
/// response, i.e. once per client session, so worst-case staleness is one
/// session. `initialize` is not a hot path and this is one indexed scan.
///
/// **Fails open.** A roster is an optimization; it must never be able to take
/// korg's MCP surface down. If the query errors, the caller emits the base
/// instructions and the session simply pays for `list_projects` the old way.
pub async fn server_instructions_with_roster(pool: &sqlx::PgPool) -> String {
    let base = server_instructions();
    match korg_core::repo::active_project_names(pool).await {
        // The pointer sentence is kept to one line on purpose: the roster is
        // paid by every session, and the names are the irreducible part of it
        // (~87 tokens for 27 projects). A longer explanation would cost more
        // than the thing it explains.
        Ok(names) if !names.is_empty() => format!(
            "{base}\n\nActive projects (the only valid targets for new work — an \
             archived one is refused, not filed): {}. \
             Pass one by name directly; call list_projects only when you need their \
             descriptions to choose between them.",
            names.join(", ")
        ),
        _ => base.to_string(),
    }
}
