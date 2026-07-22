// Typed client for korg-api. In dev, Vite proxies /api -> korg-api; in prod
// korg-api serves this bundle, so same-origin /api works directly.

export const PROJECT_STATUSES = [
  "active",
  "maintenance",
  "inactive",
  "archived",
] as const;
export type ProjectStatus = (typeof PROJECT_STATUSES)[number];

export interface Project {
  id: number;
  name: string;
  gh_repo: string | null;
  cn_path: string | null;
  description: string | null;
  status: ProjectStatus;
  machines: string[];
  deploy_to: string[];
  category: string | null;
}

/** PATCH /api/projects/:name — everything but the name (WI #246). */
export interface ProjectPatch {
  gh_repo?: string | null;
  cn_path?: string | null;
  description?: string | null;
  status?: ProjectStatus;
  machines?: string[];
  deploy_to?: string[];
  category?: string | null;
}

export interface WorkItem {
  wi_number: number;
  node_id: number;
  project: string | null;
  area: string | null;
  wi_type: string;
  wi_status: string;
  wi_tshirt: string;
  sprint: string | null;
  title: string;
  content: string;
  details: string | null;
  category: string | null;
  tags: string[];
  parent: number | null;
  archived: boolean;
  comment_count: number;
  created: string;
  updated: string;
}

export const CARD_STATUSES = [
  "Backlog",
  "Research",
  "OnDeck",
  "Active",
  "Done",
  "Cut",
] as const;
export type CardStatus = (typeof CARD_STATUSES)[number];

export interface Card {
  node_id: number;
  status: CardStatus;
  title: string;
  description: string;
  rank: string; // Decimal serialized as string
  project: string | null;
  category: string | null;
  tags: string[];
  archived: boolean;
  created: string;
  updated: string;
}

export const DISPOSITIONS = [
  "Unread",
  "Done",
  "Revisit",
  "Summarized",
  "VaultSaved",
] as const;
export type Disposition = (typeof DISPOSITIONS)[number];

export interface Link {
  node_id: number;
  url: string;
  title: string | null;
  read: boolean;
  disposition: Disposition;
  category: string | null;
  tags: string[];
}

export type DailyPlanSourceKind = "workitem" | "card" | "topic";

export interface Topic {
  node_id: number;
  name: string;
  description: string | null;
  project_id: number | null;
  project: string | null;
  category: string | null;
  tags: string[];
  archived: boolean;
  created: string;
  updated: string;
}

export interface TopicPatch {
  name?: string;
  description?: string | null;
  category?: string | null;
  tags?: string[];
}

export interface DailyPlanItem {
  node_id: number;
  plan_date: string;
  position: number;
  display: string;
  source_node_id: number;
  source_kind: DailyPlanSourceKind;
  source_title: string;
  completed_at: string | null;
  created_at: string;
}

export interface DailyPlanHistory {
  from: string;
  to: string;
  total: number;
  completed: number;
  completion_rate: number;
  items: DailyPlanItem[];
}

export type HistoryPreset = "week" | "month" | "90days" | "year";

export interface DailyPlanMoveOutcome {
  node_id: number;
  copied: boolean;
}

export interface Neighbor {
  rel_id: number;
  node_id: number;
  kind: string;
  label: string;
  /** "out" = queried node is the edge's left (label reads queried → neighbor); "in" = reverse. */
  direction: "out" | "in";
  /** False for registry-undirected labels (related-to): read the edge symmetrically. */
  directed: boolean;
}

/** GET /api/nodes/:id/neighbors — bounded, so `truncated` is explicit. */
export interface NeighborPage {
  items: Neighbor[];
  total: number;
  limit: number;
  truncated: boolean;
}

/** /api/projects/:name/plan — edges are [left, right]: left depends_on right. */
export interface PlanResponse {
  items: WorkItem[];
  edges: [number, number][];
}

/** One label/value metadata row in a node preview. */
export interface NodeField {
  label: string;
  value: string;
}

/**
 * Kind-agnostic preview of any node (WI #260). `wi_number` is set only for
 * work items (it equals node_id) — the UI navigates to those instead of
 * previewing. `body`/`details` are markdown.
 */
export interface NodePreview {
  node_id: number;
  kind: string;
  wi_number: number | null;
  title: string;
  project: string | null;
  tags: string[];
  archived: boolean;
  badges: string[];
  fields: NodeField[];
  body: string | null;
  body_label: string | null;
  details: string | null;
  created: string;
  updated: string;
}

export const PROPOSAL_STATUSES = [
  "proposed",
  "active",
  "done",
  "declined",
] as const;
export type ProposalStatus = (typeof PROPOSAL_STATUSES)[number];

export interface Proposal {
  node_id: number;
  title: string;
  summary: string;
  status: ProposalStatus;
  rank: string; // Decimal serialized as string
  pinned: boolean;
  project: string | null;
  category: string | null;
  tags: string[];
  archived: boolean;
  created: string;
  updated: string;
}

export const WI_TYPES = [
  "task",
  "bug",
  "idea",
  "research",
  "tweak",
  "issue",
  "feature",
  "epic",
  "story",
] as const;
// Canonical lifecycle (WI #285): open → resolved (implemented, may need a
// user test / PR) → done (agent satisfied; terminal but listed by default)
// → closed (Ken only; hidden by default). The server rejects other values.
export const WI_STATUSES = ["open", "resolved", "done", "closed"] as const;
export const TSHIRTS = ["XS", "S", "M", "L", "XL", "Huge", "Unknown"] as const;

export interface Comment {
  id: number;
  node_id: number;
  body: string;
  created: string;
  updated: string;
}

async function http<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const res = await fetch(path, {
    method,
    headers:
      body !== undefined ? { "content-type": "application/json" } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) {
    let detail = res.statusText;
    try {
      const j = await res.json();
      if (j && typeof j.error === "string") detail = j.error;
    } catch {
      /* ignore */
    }
    throw new Error(`${method} ${path} failed: ${detail}`);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

// Single-item reads answer 404 for "no such thing" (D-6). Callers that treat
// absence as a normal outcome (find-by-ID, refresh-after-edit) use this and
// get null; every other failure still throws.
async function httpMaybe<T>(method: string, path: string): Promise<T | null> {
  const res = await fetch(path, { method });
  if (res.status === 404) return null;
  if (!res.ok) {
    let detail = res.statusText;
    try {
      const j = await res.json();
      if (j && typeof j.error === "string") detail = j.error;
    } catch {
      /* ignore */
    }
    throw new Error(`${method} ${path} failed: ${detail}`);
  }
  return (await res.json()) as T;
}

export interface ReportRow {
  node_id: number;
  source: string;
  report_date: string;
  status: "ok" | "attention" | "problem";
  summary: string;
  model: string | null;
  escalated: boolean;
  updated: string;
}

export interface ReportFull extends ReportRow {
  body: string;
  findings: { wi_number: number; title: string; wi_status: string }[];
}

export const api = {
  // daily reports
  reports: (source?: string) =>
    http<ReportRow[]>(
      "GET",
      source
        ? `/api/reports?source=${encodeURIComponent(source)}`
        : "/api/reports",
    ),
  report: (node_id: number) =>
    http<ReportFull>("GET", `/api/reports/${node_id}`),

  // projects
  projects: () => http<Project[]>("GET", "/api/projects"),
  recentProject: () =>
    http<{ project: string | null }>("GET", "/api/projects/recent"),
  createProject: (name: string) =>
    http<{ id: number; name: string }>("POST", "/api/projects", { name }),
  updateProject: (name: string, patch: ProjectPatch) =>
    http<Project>(
      "PATCH",
      `/api/projects/${encodeURIComponent(name)}`,
      patch,
    ),

  // work items
  workItems: (project?: string) =>
    http<WorkItem[]>(
      "GET",
      project
        ? `/api/work-items?project=${encodeURIComponent(project)}`
        : "/api/work-items",
    ),
  workItem: (wi: number) =>
    httpMaybe<WorkItem>("GET", `/api/work-items/${wi}`),
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
  }) =>
    http<WorkItem>("POST", "/api/work-items", b),
  updateWorkItem: (
    wi: number,
    patch: Partial<{
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
      tags: string[];
    }>,
  ) => http<WorkItem>("PATCH", `/api/work-items/${wi}`, patch),
  areas: (project: string) =>
    http<{ id: number; name: string }[]>(
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
  cards: () => http<Card[]>("GET", "/api/cards"),
  createCard: (b: { title: string; status?: CardStatus; rank?: number }) =>
    http<Card>("POST", "/api/cards", b),
  updateCard: (
    node_id: number,
    patch: Partial<{
      status: CardStatus;
      rank: number;
      title: string;
      description: string;
      archived: boolean;
      project: string | null;
      category: string | null;
      tags: string[];
    }>,
  ) => http<Card>("PATCH", `/api/cards/${node_id}`, patch),
  nodeComments: (node_id: number) =>
    http<Comment[]>("GET", `/api/nodes/${node_id}/comments`),
  addComment: (node_id: number, body: string) =>
    http<Comment>("POST", `/api/nodes/${node_id}/comments`, { body }),
  updateComment: (id: number, body: string) =>
    http<Comment>("PATCH", `/api/comments/${id}`, { body }),
  deleteComment: (id: number) =>
    http<{ deleted: boolean }>("DELETE", `/api/comments/${id}`),

  // reading-list links
  links: () => http<Link[]>("GET", "/api/links"),
  createLink: (b: { url: string; title?: string; tags?: string[] }) =>
    http<Link>("POST", "/api/links", b),
  updateLink: (
    node_id: number,
    patch: Partial<{ disposition: Disposition; read: boolean; tags: string[] }>,
  ) => http<Link>("PATCH", `/api/links/${node_id}`, patch),

  // topics
  topics: (query?: string) =>
    http<Topic[]>(
      "GET",
      query === undefined
        ? "/api/topics"
        : `/api/topics?q=${encodeURIComponent(query)}`,
    ),
  topic: (node_id: number) => httpMaybe<Topic>("GET", `/api/topics/${node_id}`),
  createTopic: (body: {
    name: string;
    description?: string;
    project_id?: number;
    category?: string;
    tags?: string[];
  }) => http<Topic>("POST", "/api/topics", body),
  updateTopic: (node_id: number, patch: TopicPatch) =>
    http<Topic>("PATCH", `/api/topics/${node_id}`, patch),
  archiveTopic: (node_id: number, archived = true) =>
    http<Topic>("POST", `/api/topics/${node_id}/archive`, { archived }),

  // daily planning
  dailyPlan: (from: string, to: string) =>
    http<DailyPlanItem[]>(
      "GET",
      `/api/daily-plan?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`,
    ),
  createDailyPlanItem: (source_node_id: number, plan_date: string) =>
    http<{ node_id: number }>("POST", "/api/daily-plan", {
      source_node_id,
      plan_date,
    }),
  setDailyPlanCompletion: (node_id: number, completed: boolean) =>
    http<{ ok: true }>("PATCH", `/api/daily-plan/${node_id}/completion`, {
      completed,
    }),
  deleteDailyPlanItem: (node_id: number) =>
    http<{ deleted: boolean }>("DELETE", `/api/daily-plan/${node_id}`),
  moveDailyPlanItem: (
    node_id: number,
    target_date: string,
    target_position = 0,
  ) =>
    http<DailyPlanMoveOutcome>("POST", `/api/daily-plan/${node_id}/move`, {
      target_date,
      target_position,
    }),
  reorderDailyPlan: (plan_date: string, node_ids: number[]) =>
    http<{ ok: true }>(
      "PUT",
      `/api/daily-plan/${encodeURIComponent(plan_date)}/order`,
      {
        node_ids,
      },
    ),
  dailyPlanHistory: (preset: HistoryPreset, source_node_id?: number) => {
    const params = new URLSearchParams({ preset });
    if (source_node_id !== undefined)
      params.set("source_node_id", String(source_node_id));
    return http<DailyPlanHistory>("GET", `/api/daily-plan/history?${params}`);
  },

  // relationships
  relate: (left: number, right: number, label: string) =>
    http<{ id: number }>("POST", "/api/relationships", { left, right, label }),
  unrelate: (id: number) =>
    http<{ deleted: boolean }>("DELETE", `/api/relationships/${id}`),
  neighbors: (id: number, opts?: { label?: string; kind?: string; limit?: number }) => {
    const p = new URLSearchParams();
    if (opts?.label) p.set("label", opts.label);
    if (opts?.kind) p.set("kind", opts.kind);
    if (opts?.limit !== undefined) p.set("limit", String(opts.limit));
    const qs = p.toString();
    return http<NeighborPage>(
      "GET",
      `/api/nodes/${id}/neighbors${qs ? `?${qs}` : ""}`,
    );
  },
  node: (id: number) => httpMaybe<NodePreview>("GET", `/api/nodes/${id}`),
  plan: (project: string) =>
    http<PlanResponse>(
      "GET",
      `/api/projects/${encodeURIComponent(project)}/plan`,
    ),

  // sprint proposals (agent planning)
  proposals: (status?: ProposalStatus) =>
    http<Proposal[]>(
      "GET",
      status ? `/api/proposals?status=${status}` : "/api/proposals",
    ),
  createProposal: (b: {
    title: string;
    summary: string;
    work_item_numbers?: number[];
    project_id?: number;
    rank?: number;
    pinned?: boolean;
    tags?: string[];
  }) =>
    http<Proposal & { covered: number[] }>("POST", "/api/proposals", b),
  updateProposal: (
    node_id: number,
    patch: Partial<{
      title: string;
      summary: string;
      status: ProposalStatus;
      rank: number;
      pinned: boolean;
      archived: boolean;
      tags: string[];
    }>,
  ) => http<Proposal>("PATCH", `/api/proposals/${node_id}`, patch),
};
