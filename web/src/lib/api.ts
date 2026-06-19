// Typed client for korg-api. In dev, Vite proxies /api -> korg-api; in prod
// korg-api serves this bundle, so same-origin /api works directly.

export interface Project {
  id: number;
  name: string;
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
  archived: boolean;
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

export interface Slot {
  node_id: number;
  slot_date: string; // YYYY-MM-DD
  duration_minutes: number;
  label: string | null;
  goal: string | null;
  position: number;
}

export interface Neighbor {
  node_id: number;
  kind: string;
  label: string;
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
export const WI_STATUSES = ["open", "active", "resolved", "closed", "draft"] as const;
export const TSHIRTS = ["XS", "S", "M", "L", "XL", "Huge", "Unknown"] as const;

async function http<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: body !== undefined ? { "content-type": "application/json" } : undefined,
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

export const api = {
  // projects
  projects: () => http<Project[]>("GET", "/api/projects"),
  recentProject: () => http<{ project: string | null }>("GET", "/api/projects/recent"),
  createProject: (name: string) => http<{ id: number; name: string }>("POST", "/api/projects", { name }),

  // work items
  workItems: (project?: string) =>
    http<WorkItem[]>(
      "GET",
      project ? `/api/work-items?project=${encodeURIComponent(project)}` : "/api/work-items",
    ),
  workItem: (wi: number) => http<WorkItem | null>("GET", `/api/work-items/${wi}`),
  createWorkItem: (b: {
    title: string;
    content: string;
    wi_type?: string;
    wi_status?: string;
    wi_tshirt?: string;
    project_id?: number;
  }) => http<{ node_id: number; wi_number: number }>("POST", "/api/work-items", b),

  // cards
  cards: () => http<Card[]>("GET", "/api/cards"),
  createCard: (b: { title: string; status?: CardStatus; rank?: number }) =>
    http<{ node_id: number }>("POST", "/api/cards", b),
  updateCard: (
    node_id: number,
    patch: Partial<{
      status: CardStatus;
      rank: number;
      title: string;
      description: string;
      archived: boolean;
      tags: string[];
    }>,
  ) => http<{ ok: true }>("PATCH", `/api/cards/${node_id}`, patch),

  // reading-list links
  links: () => http<Link[]>("GET", "/api/links"),
  createLink: (b: { url: string; title?: string; tags?: string[] }) =>
    http<{ node_id: number }>("POST", "/api/links", b),
  updateLink: (
    node_id: number,
    patch: Partial<{ disposition: Disposition; read: boolean; tags: string[] }>,
  ) => http<{ ok: true }>("PATCH", `/api/links/${node_id}`, patch),

  // slots
  slots: (from: string, to: string) =>
    http<Slot[]>("GET", `/api/slots?from=${from}&to=${to}`),
  generateSlots: (start: string, days: number) =>
    http<{ created: number }>("POST", "/api/slots/generate", { start, days }),
  setSlotGoal: (node_id: number, goal: string | null) =>
    http<{ ok: true }>("PATCH", `/api/slots/${node_id}`, { goal }),

  // relationships
  relate: (left: number, right: number, label: string) =>
    http<{ id: number }>("POST", "/api/relationships", { left, right, label }),
  neighbors: (id: number) => http<Neighbor[]>("GET", `/api/nodes/${id}/neighbors`),
};
