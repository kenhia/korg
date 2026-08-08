// Typed client for korg-api. In dev, Vite proxies /api -> korg-api; in prod
// korg-api serves this bundle, so same-origin /api works directly.
//
// This file holds fetch wrappers and nothing else (WI #541). Every shape it
// mentions comes from `./generated/`, which `just gen` derives from korg-core —
// the ~500 lines of hand-mirrored interfaces that used to live here had already
// drifted from the server (WorkItemRow statuses typed `string` while CardRow and
// ProposalRow used unions; create/update shapes narrower than the API actually
// accepts; a nine-entry WI_TYPES list of which the server rejects six).

import type {
  AreaRow,
  AwaitingRow,
  CardRow,
  Comment,
  HandoffFull,
  LinkRow,
  NeighborPage,
  NodePreview,
  Page,
  PlanningRollupRow,
  ProgramDetail,
  ProgramList,
  ProgramRow,
  ProjectRow,
  ProposalDetail,
  ProposalRow,
  ReportFull,
  ReportRow,
  ScheduleDetail,
  ScheduleList,
  ScheduleRow,
  SourceHealth,
  WorkItemDetail,
  WorkItemListLean,
  WorkItemRow,
} from "./generated/korg";
import type {
  CardStatus,
  Disposition,
  ErrorCode,
  ProgramStatus,
  ProposalStatus,
  ScheduleAnchor,
  ScheduleCadence,
  ScheduleStatus,
  WiTshirt,
  WiType,
} from "./generated/vocab";
import { ERROR_CODES } from "./generated/vocab";

export type * from "./generated/korg";
export type * from "./generated/vocab";

/**
 * A failed API call, with the server's classification intact.
 *
 * korg's REST errors are `{error, code}` where `code` is one of
 * `invalid_input | not_found | conflict | internal` (sprint 013, D-5). Until
 * sprint 019 this client flattened both into one string, so every caller that
 * wanted to behave differently for "you typed something wrong" than for "korg
 * fell over" had no way to tell — the whole point of adding `code` was lost in
 * the last five lines before it reached the UI.
 *
 * `detail` is the server's own sentence, which is written for a person
 * ("no project named 'KORG' — did you mean 'korg'?"). Show that. `method` and
 * `path` are kept as fields for the console rather than being prepended to the
 * message, because a log line is not a user-facing string.
 */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: ErrorCode | null,
    readonly detail: string,
    readonly method: string,
    readonly path: string,
  ) {
    super(detail);
    this.name = "ApiError";
  }

  /** The caller supplied something korg refused — the user can fix it. */
  get isUserFixable(): boolean {
    return this.code === "invalid_input" || this.code === "conflict";
  }
}

/** A network failure — the request never got an answer. Distinct from an
 *  `ApiError`, which means korg replied and said no. */
export class NetworkError extends Error {
  constructor(
    readonly method: string,
    readonly path: string,
    readonly cause: unknown,
  ) {
    super("Could not reach korg — check that the server is running.");
    this.name = "NetworkError";
  }
}

function isErrorCode(v: unknown): v is ErrorCode {
  return typeof v === "string" && (ERROR_CODES as readonly string[]).includes(v);
}

/** The plan payload: a project's items plus its `depends_on` edges,
 *  `[left, right]` = left depends on right. Assembled by the handler rather
 *  than a core struct, so it is declared here. */
export interface PlanResponse {
  items: WorkItemRow[];
  edges: [number, number][];
}

/** Shared collection-read params. `archived` omitted = unarchived only (D-3).
 *  A query string cannot carry JSON `null`, so REST spells the tri-state as
 *  these three words — see the note in korg-core's `ops` module. */
export interface ListParams {
  archived?: "true" | "false" | "all";
  limit?: number;
  offset?: number;
}


/** `korg_core::repo::LIST_LIMIT_MAX` — the largest page any collection read
 *  will serve, whatever you ask for. Not generated: it is a repo constant, not
 *  part of a shared operation struct. */
export const LIST_LIMIT_MAX = 500;

/** How many pages `allWorkItems` fetches before it gives up and says so.
 *  2000 rows at `LIST_LIMIT_MAX` — far above the corpus (572 on 2026-08-01) and
 *  low enough that a runaway collection cannot turn "load everything" into a
 *  page that is merely slow, with no signal (WI #762, D-2). */
export const AUTO_PAGE_LIMIT = 4;

/** A collection read walked to completion — or as far as the page bound allowed.
 *
 *  `items.length`, `total` and the caller's own filtered count are three
 *  different numbers, and conflating them is the bug this type exists to stop:
 *  the Work Items footer used to show the filtered count of an arbitrary first
 *  page and call it "N items".
 */
export interface WalkedPage<T> {
  items: T[];
  /** What the server says matches the query, ignoring client-side filtering. */
  total: number;
  /** False when the page bound stopped the walk short of `total`. */
  complete: boolean;
}

function listQuery(
  params: Record<string, string | number | boolean | undefined>,
): string {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== "") p.set(k, String(v));
  }
  const qs = p.toString();
  return qs ? `?${qs}` : "";
}

async function failure(method: string, path: string, res: Response) {
  let detail = res.statusText;
  let code: ErrorCode | null = null;
  try {
    const j = await res.json();
    if (j && typeof j.error === "string") detail = j.error;
    if (j && isErrorCode(j.code)) code = j.code;
  } catch {
    /* a non-JSON body (proxy error page, empty 502) leaves statusText */
  }
  return new ApiError(res.status, code, detail, method, path);
}

/** `fetch` itself only rejects when the request never completed. Everything
 *  else — 404, 500, a proxy's HTML error page — comes back as a `Response`. */
async function send(
  method: string,
  path: string,
  init: RequestInit,
): Promise<Response> {
  try {
    return await fetch(path, init);
  } catch (cause) {
    throw new NetworkError(method, path, cause);
  }
}

async function http<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const res = await send(method, path, {
    method,
    headers:
      body !== undefined ? { "content-type": "application/json" } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw await failure(method, path, res);
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

// Single-item reads answer 404 for "no such thing" (D-6). Callers that treat
// absence as a normal outcome (find-by-ID, refresh-after-edit) use this and
// get null; every other failure still throws.
async function httpMaybe<T>(method: string, path: string): Promise<T | null> {
  const res = await send(method, path, { method });
  if (res.status === 404) return null;
  if (!res.ok) throw await failure(method, path, res);
  return (await res.json()) as T;
}

/** Patch bodies are partial by construction: every field is "leave unchanged"
 *  when omitted. The server's patch structs say the same thing with `Option`. */
type Patch<T> = Partial<T>;

export const api = {
  // daily reports
  reports: (source?: string) =>
    http<ReportRow[]>("GET", `/api/reports${listQuery({ source })}`),
  report: (node_id: number) =>
    http<ReportFull>("GET", `/api/reports/${node_id}`),

  // projects
  projects: () => http<ProjectRow[]>("GET", "/api/projects"),
  recentProject: () =>
    http<{ project: string | null }>("GET", "/api/projects/recent"),
  createProject: (name: string) =>
    http<{ id: number; name: string }>("POST", "/api/projects", { name }),
  updateProject: (
    name: string,
    patch: Patch<{
      gh_repo: string | null;
      src_path: string | null;
      description: string | null;
      notes: string | null;
      status: string;
      machines: string[];
      deploy_to: string[];
      category: string | null;
    }>,
  ) =>
    http<ProjectRow>(
      "PATCH",
      `/api/projects/${encodeURIComponent(name)}`,
      patch,
    ),

  // work items
  workItems: (project?: string, params: ListParams = {}) =>
    http<Page<WorkItemRow>>(
      "GET",
      `/api/work-items${listQuery({ project, ...params, limit: params.limit ?? LIST_LIMIT_MAX })}`,
    ),
  /** Every work item matching the query, not the first `LIST_LIMIT_MAX` of them
   *  (WI #762). Both list views hold their whole collection and filter it in
   *  memory, so a page that silently ends at 500 is a page whose filters,
   *  counts and tag chips are all quietly wrong — and on the ascending
   *  `wi_number` order it drops the *newest* rows.
   *
   *  Walks by offset rather than re-reading from 0, which is stable here: the
   *  order is `wi_number` ascending and new rows take higher numbers, so a
   *  concurrent create appends past the walk instead of shifting it. Rows are
   *  archived, never deleted, and `archived: "all"` keeps even those in place.
   *
   *  Stops after `maxPages` and reports `complete: false` — see AUTO_PAGE_LIMIT.
   */
  allWorkItems: async (
    project?: string,
    params: ListParams = {},
    maxPages = AUTO_PAGE_LIMIT,
  ): Promise<WalkedPage<WorkItemRow>> => {
    const limit = params.limit ?? LIST_LIMIT_MAX;
    let offset = params.offset ?? 0;
    const items: WorkItemRow[] = [];
    let total = 0;
    for (let fetched = 0; fetched < maxPages; fetched++) {
      const page = await api.workItems(project, { ...params, limit, offset });
      total = page.total;
      items.push(...page.items);
      offset += page.items.length;
      // An empty page means the server has nothing more to give, whatever
      // `total` claims — without this a disagreement between the two would
      // spin until maxPages.
      if (page.items.length === 0 || items.length >= total) break;
    }
    return { items, total, complete: items.length >= total };
  },
  /** The slim projection, and the only work-item read with a *server-side*
   *  status filter. The Review page (WI #570) needs "every done/resolved item"
   *  to be a complete answer, which filtering `workItems` in the client cannot
   *  give: that read is capped at LIST_LIMIT_MAX and the cap is spent on rows
   *  of every status (WI #762). Asking the server for the two statuses costs
   *  two requests and can only truncate on 500 items *of those statuses*.
   *
   *  Since WI #861 this shares one core read with the MCP `list_work_items`,
   *  so omitting `wi_status` means "everything not terminal" rather than
   *  "every status" — pass `"all"` for the old behaviour. Every caller here
   *  passes a status explicitly. */
  surveyWorkItems: (
    params: {
      project?: string;
      wi_status?: string;
      archived?: boolean;
      limit?: number;
      offset?: number;
    } = {},
  ) =>
    http<WorkItemListLean>(
      "GET",
      `/api/work-items/survey${listQuery({ ...params, limit: params.limit ?? 500 })}`,
    ),
  workItem: (wi: number) =>
    httpMaybe<WorkItemDetail>("GET", `/api/work-items/${wi}`),
  createWorkItem: (b: {
    title: string;
    content: string;
    wi_type?: string;
    wi_status?: string;
    wi_tshirt?: string;
    sprint?: string;
    details?: string;
    area_id?: number;
    project_id?: number;
  }) => http<WorkItemRow>("POST", "/api/work-items", b),
  updateWorkItem: (
    wi: number,
    patch: Patch<{
      title: string;
      content: string;
      details: string | null;
      wi_type: string;
      wi_status: string;
      wi_tshirt: string;
      sprint: string | null;
      project_id: number | null;
      area_id: number | null;
      parent: number | null;
      archived: boolean;
      category: string | null;
      tags: string[];
    }>,
  ) => http<WorkItemRow>("PATCH", `/api/work-items/${wi}`, patch),
  areas: (project: string) =>
    http<AreaRow[]>(
      "GET",
      `/api/areas?project=${encodeURIComponent(project)}`,
    ),
  createArea: (project: string, name: string, description?: string) =>
    http<{ id: number; name: string }>("POST", "/api/areas", {
      project,
      name,
      description,
    }),

  // cards
  cards: (params: ListParams & { status?: string; project?: string } = {}) =>
    http<Page<CardRow>>(
      "GET",
      `/api/cards${listQuery({ ...params, limit: params.limit ?? 500 })}`,
    ),
  createCard: (b: { title: string; status?: CardStatus; rank?: number }) =>
    http<CardRow>("POST", "/api/cards", b),
  updateCard: (
    node_id: number,
    patch: Patch<{
      status: CardStatus;
      rank: number;
      title: string;
      description: string;
      archived: boolean;
      project_id: number | null;
      category: string | null;
      tags: string[];
    }>,
  ) => http<CardRow>("PATCH", `/api/cards/${node_id}`, patch),

  // comments
  nodeComments: (node_id: number) =>
    http<Comment[]>("GET", `/api/nodes/${node_id}/comments`),
  addComment: (node_id: number, body: string) =>
    http<Comment>("POST", `/api/nodes/${node_id}/comments`, { body }),
  updateComment: (id: number, body: string) =>
    http<Comment>("PATCH", `/api/comments/${id}`, { body }),
  deleteComment: (id: number) =>
    http<{ deleted: boolean }>("DELETE", `/api/comments/${id}`),

  // reading-list links
  links: (params: ListParams & { disposition?: string; read?: boolean } = {}) =>
    http<Page<LinkRow>>(
      "GET",
      `/api/links${listQuery({ ...params, limit: params.limit ?? 500 })}`,
    ),
  createLink: (b: { url: string; title?: string; tags?: string[] }) =>
    http<LinkRow>("POST", "/api/links", b),
  /** One transactional update — disposition, read and tags together (WI #538). */
  updateLink: (
    node_id: number,
    patch: Patch<{ disposition: Disposition; read: boolean; tags: string[] }>,
  ) => http<LinkRow>("PATCH", `/api/links/${node_id}`, patch),

  // relationships
  relate: (left: number, right: number, label: string) =>
    http<{ id: number }>("POST", "/api/relationships", { left, right, label, origin: "web" }),
  unrelate: (id: number) =>
    http<{ deleted: boolean }>("DELETE", `/api/relationships/${id}`),
  neighbors: (
    id: number,
    opts: { label?: string; kind?: string; limit?: number } = {},
  ) =>
    http<NeighborPage>(
      "GET",
      `/api/nodes/${id}/neighbors${listQuery({ ...opts })}`,
    ),
  node: (id: number) => httpMaybe<NodePreview>("GET", `/api/nodes/${id}`),
  plan: (project: string) =>
    http<PlanResponse>(
      "GET",
      `/api/projects/${encodeURIComponent(project)}/plan`,
    ),

  // sprint proposals (agent planning)
  proposals: (status?: ProposalStatus, project?: string) =>
    http<ProposalRow[]>("GET", `/api/proposals${listQuery({ status, project })}`),
  proposal: (node_id: number) =>
    httpMaybe<ProposalDetail>("GET", `/api/proposals/${node_id}`),
  // Per-project planning weather for the Planning rail (WI #823). One call for
  // every project — the rail renders ~30 of them, and a per-project read is
  // the N+1 this sprint exists to delete.
  planningRollup: () =>
    http<PlanningRollupRow[]>("GET", "/api/proposals/rollup"),
  updateProposal: (
    node_id: number,
    patch: Patch<{
      title: string;
      summary: string;
      status: ProposalStatus;
      rank: number;
      pinned: boolean;
      archived: boolean;
      tags: string[];
    }>,
  ) => http<ProposalRow>("PATCH", `/api/proposals/${node_id}`, patch),

  // handoffs. The authoritative read: body plus the nodes it is attached to,
  // which is strictly more than the generic /api/nodes/:id preview carries —
  // the reason the reading page (WI #621) has its own route rather than a
  // generic one. Read-only from the web; authoring is API/skill-driven.
  handoff: (node_id: number) =>
    httpMaybe<HandoffFull>("GET", `/api/handoffs/${node_id}`),

  // programs — the cross-project layer (#968). Note there is no project filter:
  // a program has no project of its own, and each row carries the `span`
  // derived from its slices instead.
  programs: (status?: ProgramStatus | "all") =>
    http<ProgramList>("GET", `/api/programs${listQuery({ status })}`),
  // One call renders the whole page: slices in order, each with its work-item
  // rollup. No per-slice fetch — that is the point of the read.
  program: (node_id: number) =>
    httpMaybe<ProgramDetail>("GET", `/api/programs/${node_id}`),
  updateProgram: (
    node_id: number,
    patch: Patch<{
      title: string;
      aim: string;
      notes: string | null;
      status: ProgramStatus;
      rank: number;
      pinned: boolean;
      archived: boolean;
      tags: string[];
    }>,
  ) => http<ProgramRow>("PATCH", `/api/programs/${node_id}`, patch),

  // schedules — work that a date makes appear (#581). `due` is computed on
  // every read, so nothing here caches it; a stale due flag over a clock is
  // precisely the failure this feature exists to remove.
  schedules: (opts?: {
    status?: ScheduleStatus | "all";
    project?: string;
    due_only?: boolean;
  }) =>
    http<ScheduleList>(
      "GET",
      `/api/schedules${listQuery({
        status: opts?.status,
        project: opts?.project,
        due_only: opts?.due_only ? "true" : undefined,
      })}`,
    ),
  schedule: (node_id: number) =>
    httpMaybe<ScheduleDetail>("GET", `/api/schedules/${node_id}`),
  updateSchedule: (
    node_id: number,
    patch: Patch<{
      title: string;
      template: string | null;
      notes: string | null;
      cadence: ScheduleCadence;
      anchor_mode: ScheduleAnchor;
      anchor_at: string;
      status: ScheduleStatus;
      wi_type: WiType;
      wi_tshirt: WiTshirt;
      archived: boolean;
      tags: string[];
    }>,
  ) => http<ScheduleRow>("PATCH", `/api/schedules/${node_id}`, patch),
  /** Turn a due schedule into a work item. `force` brings a not-yet-due one
   *  forward; it never lifts the outstanding-item refusal. */
  materializeSchedule: (node_id: number, force = false) =>
    http<{ work_item: WorkItemRow; schedule: ScheduleRow }>(
      "POST",
      `/api/schedules/${node_id}/materialize${force ? "?force=true" : ""}`,
    ),

  // report-source staleness (#950). A bare array by design — one row per
  // source, and capping it could push a stale source off the end behind
  // fresher ones, which inverts the failure it exists to catch.
  reportSources: () => http<SourceHealth[]>("GET", "/api/report-sources"),
  setReportSource: (
    source: string,
    patch: Patch<{
      cadence_days: number | null;
      grace_days: number | null;
      retired: boolean;
      note: string | null;
    }>,
  ) => http<SourceHealth>("PATCH", `/api/report-sources/${source}`, patch),

  // the awaiting-Ken lane (#969). `setAwaiting(id, false)` is the one-click
  // clear — the same core path an agent uses, not a second one that could
  // drift from it.
  awaiting: () => http<AwaitingRow[]>("GET", "/api/awaiting"),
  setAwaiting: (node_id: number, awaiting: boolean, note?: string) =>
    http<AwaitingRow>("PUT", `/api/nodes/${node_id}/awaiting`, {
      awaiting,
      ...(awaiting && note ? { note } : {}),
    }),
};
