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
  CardRow,
  Comment,
  DailyPlanItem,
  History,
  LinkRow,
  MoveOutcome,
  NeighborPage,
  NodePreview,
  Page,
  ProjectRow,
  ProposalDetail,
  ProposalRow,
  ReportFull,
  ReportRow,
  Topic,
  WorkItemDetail,
  WorkItemRow,
} from "./generated/korg";
import type {
  CardStatus,
  Disposition,
  ErrorCode,
  ProposalStatus,
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

export type HistoryPreset = "week" | "month" | "90days" | "year";

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
      cn_path: string | null;
      description: string | null;
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
      `/api/work-items${listQuery({ project, ...params, limit: params.limit ?? 500 })}`,
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

  // topics
  topics: (query?: string, params: ListParams = {}) =>
    http<Page<Topic>>(
      "GET",
      `/api/topics${listQuery({ q: query, ...params, limit: params.limit ?? 500 })}`,
    ),
  topic: (node_id: number) => httpMaybe<Topic>("GET", `/api/topics/${node_id}`),
  createTopic: (body: {
    name: string;
    description?: string;
    project_id?: number;
    category?: string;
    tags?: string[];
  }) => http<Topic>("POST", "/api/topics", body),
  updateTopic: (
    node_id: number,
    patch: Patch<{
      name: string;
      description: string | null;
      category: string | null;
      tags: string[];
    }>,
  ) => http<Topic>("PATCH", `/api/topics/${node_id}`, patch),
  archiveTopic: (node_id: number, archived = true) =>
    http<Topic>("POST", `/api/topics/${node_id}/archive`, { archived }),

  // daily planning
  dailyPlan: (from: string, to: string) =>
    http<DailyPlanItem[]>("GET", `/api/daily-plan${listQuery({ from, to })}`),
  createDailyPlanItem: (source_node_id: number, plan_date: string) =>
    http<DailyPlanItem>("POST", "/api/daily-plan", {
      source_node_id,
      plan_date,
    }),
  setDailyPlanCompletion: (node_id: number, completed: boolean) =>
    http<DailyPlanItem>("PATCH", `/api/daily-plan/${node_id}/completion`, {
      completed,
    }),
  deleteDailyPlanItem: (node_id: number) =>
    http<{ deleted: boolean }>("DELETE", `/api/daily-plan/${node_id}`),
  moveDailyPlanItem: (
    node_id: number,
    target_date: string,
    target_position = 0,
  ) =>
    http<MoveOutcome>("POST", `/api/daily-plan/${node_id}/move`, {
      target_date,
      target_position,
    }),
  dailyPlanHistory: (preset: HistoryPreset, source_node_id?: number) =>
    http<History>(
      "GET",
      `/api/daily-plan/history${listQuery({ preset, source_node_id })}`,
    ),

  // relationships
  relate: (left: number, right: number, label: string) =>
    http<{ id: number }>("POST", "/api/relationships", { left, right, label }),
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
};
