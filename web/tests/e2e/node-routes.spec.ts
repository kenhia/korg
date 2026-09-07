import { test, expect } from "@playwright/test";
import { NODE_KINDS } from "../../src/lib/generated/vocab";
import { docTitle } from "../../src/lib/domain";

// Sprint 070 (#1467) — every node kind is reachable at a URL.
//
// The gap these cover was invisible on the board because the consumers hitting
// it were degrading correctly rather than breaking: kfdc's Net Log rendered
// plain text where a kind had no page, and korg's own awaiting lane linked to a
// *list* page when it could not link to the thing. The one exception was
// `/work-items?wi=<n>`, a URL `nodeHref` produced and the Work Items page never
// read — it landed on the unfiltered list and did nothing with the parameter.
//
// So these are coverage assertions. The interesting ones are the last two: the
// resolver, which is the call a consumer holding a locator cannot make for
// itself, and the comment anchor, which is the half `nodePage` provably cannot
// reconstruct from a search hit.

test("a work item has a page of its own, and the proposal links to it", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = `e2e-routes-${stamp}`;
  const title = `addressable item ${stamp}`;

  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const wi = await (
    await request.post("/api/work-items", {
      data: { title, content: "reachable at a URL", project_id: pid },
    })
  ).json();

  await page.goto(`/work-items/${wi.wi_number}`);
  const detail = page.getByTestId("node-detail");
  await expect(detail).toBeVisible();
  await expect(detail).toContainText(title);
  await expect(page.getByTestId("node-detail-id")).toContainText(`#${wi.wi_number}`);

  // The link that used to be dead. `/work-items?wi=<n>` landed on the list.
  const proposal = await (
    await request.post("/api/proposals", {
      data: {
        title: `proposal ${stamp}`,
        summary: "covers the addressable item",
        project_id: pid,
        work_item_numbers: [wi.wi_number],
      },
    })
  ).json();
  await page.goto(`/planning/${proposal.node_id}`);
  await page.getByTestId("proposal-covered").getByRole("link").first().click();
  await expect(page).toHaveURL(new RegExp(`/work-items/${wi.wi_number}$`));
  await expect(page.getByTestId("node-detail")).toContainText(title);
});

test("a card, a link and a schedule each render at their own URL", async ({
  page,
  request,
}) => {
  const stamp = Date.now();

  const card = await (
    await request.post("/api/cards", { data: { title: `e2e card ${stamp}` } })
  ).json();
  const link = await (
    await request.post("/api/links", {
      data: { url: `https://example.invalid/${stamp}`, title: `e2e link ${stamp}` },
    })
  ).json();
  const schedule = await (
    await request.post("/api/schedules", {
      data: { title: `e2e schedule ${stamp}`, cadence: "monthly" },
    })
  ).json();

  for (const [path, text] of [
    [`/cards/${card.node_id}`, `e2e card ${stamp}`],
    [`/reading-list/${link.node_id}`, `e2e link ${stamp}`],
    [`/schedules/${schedule.node_id}`, `e2e schedule ${stamp}`],
  ] as const) {
    await page.goto(path);
    await expect(page.getByTestId("node-detail"), path).toContainText(text);
  }
});

test("a route says so when the id is a different kind", async ({ page, request }) => {
  // Node ids are one sequence across every kind, so /cards/<proposal id> is a
  // typo away. Rendering it under the Cards heading would be the wrong answer.
  const stamp = Date.now();
  const project = `e2e-wrongkind-${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title: `not a card ${stamp}`, summary: "s", project_id: pid },
    })
  ).json();

  await page.goto(`/cards/${proposal.node_id}`);
  const notice = page.getByTestId("wrong-kind");
  await expect(notice).toContainText("sprint_proposal");
  await notice.getByRole("link").click();
  await expect(page).toHaveURL(new RegExp(`/planning/${proposal.node_id}$`));
});

test("/n/:node_id resolves a locator to the node's page", async ({ page, request }) => {
  // The whole point of the resolver: the caller supplies an id and no kind.
  const stamp = Date.now();
  const project = `e2e-resolve-${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title: `resolvable ${stamp}`, summary: "s", project_id: pid },
    })
  ).json();

  await page.goto(`/n/${proposal.node_id}`);
  await expect(page).toHaveURL(new RegExp(`/planning/${proposal.node_id}$`));
  await expect(page.getByTestId("proposal-detail-id")).toContainText(
    `#${proposal.node_id}`,
  );
});

test("a comment locator lands on the comment", async ({ page, request }) => {
  // `korg:<node>#comment-<id>` is a spelling korg already prints. This is the
  // half a consumer cannot rebuild: a search hit on a comment carries the
  // owning node's id without its kind.
  const stamp = Date.now();
  const project = `e2e-anchor-${stamp}`;
  const needle = `zamboni resurfacing ${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title: `anchored ${stamp}`, summary: "s", project_id: pid },
    })
  ).json();
  // Enough comments that landing on the right one is visibly different from
  // landing at the top of the thread.
  for (let i = 0; i < 6; i++) {
    await request.post(`/api/nodes/${proposal.node_id}/comments`, {
      data: { body: `filler ${i} ${stamp}` },
    });
  }
  const target = await (
    await request.post(`/api/nodes/${proposal.node_id}/comments`, {
      data: { body: needle },
    })
  ).json();

  // korg returns the URL; the page follows it rather than deriving one.
  const hits = await (
    await request.get(`/api/search?q=${encodeURIComponent(needle)}&scope=all`)
  ).json();
  const hit = hits.items.find((h: { comment_id: number | null }) => h.comment_id === target.id);
  expect(hit, "the comment should be findable by its own body").toBeTruthy();
  expect(hit.url).toBe(`/planning/${proposal.node_id}#comment-${target.id}`);

  await page.goto(hit.url);
  const anchored = page.locator(`#comment-${target.id}`);
  await expect(anchored).toContainText(needle);
  await expect(anchored).toBeInViewport();
});

// Sprint 077 (#1966) — the tab names the node by id, on every kind's page.
//
// The title is `korg — WI 1961`, not the node's own title: what you reach for
// when a long body has scrolled the header off screen is the id. `docTitle`
// reads `NODE_TITLE_WORDS`, which korg-core generates beside `NODE_ROUTES` and
// fences against `NODE_KINDS` — so a tenth kind cannot get a page without a
// word for it. What the fence *cannot* see is whether each page actually calls
// `docTitle`, which is four separate call sites (`NodeDetail`, and the three
// kinds with bespoke pages). That is what these assert.

/** A real 16×16 PNG. The bytes are decoded server-side and both variants are
 *  generated from them, so this cannot be a stub — but it also does not need to
 *  be painted in the page the way `images-ui.spec.ts` does, because nothing
 *  here is testing the paste path. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAABlklEQVR42hXRURVEIQhFUSMYgQhG" +
    "MAIRiGCEE8EIRiACEYhABCLMG7/ZrMt1jMEcyGAN9kAHNjgDBnfwBj6IQQ5q0IMxJnMikzXZE53Y" +
    "5EyY3Mmb+CQmOalJzw8IUxBhCVtQwYQjIFzhCS6EkEIJLR9YzIUs1mIvdGGLs2BxF2/hi1jkoha9" +
    "PrCZG9mszd7oxjZnw+Zu3sY3sclNbXp/QJmKKEvZiiqmHAXlKk9xJZRUSmn9gDENMZaxDTXMOAbG" +
    "NZ7hRhhplNH2gcM8yGEd9kEPdjgHDvfwDn6IQx7q0OcD/wK/Sr4jv9hfkG/1N/x/Fx44BCQU9Pc9" +
    "4zIvclmXfdGLXc79j9/Lu/glLnmpS98PPOZDHuuxH/qwx3n/5ffxHv6IRz7q0e8DznTEWc521DHn" +
    "+D/KdZ7jTjjplNP+gWAGEqxgBxpYcOIf/AYv8CCCDCro+EAyE0lWshNNLDn5P/MmL/Ekkkwq6fxA" +
    "MQspVrELLaw49S/lFq/wIoosquj6QDMbaVazG22sOf2v8Dav8SaabKrp5geIAnAQC3NfwAAAAABJ" +
    "RU5ErkJggg==",
  "base64",
);

test("every detail page names its node by id in the tab title", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = `e2e-title-${stamp}`;
  const pid = (
    await (
      await request.post("/api/projects", { data: { name: project } })
    ).json()
  ).id as number;

  const post = async (path: string, data: unknown) => {
    const res = await request.post(path, { data });
    expect(res.ok(), `${path} should accept the fixture`).toBeTruthy();
    return res.json();
  };

  const wi = await post("/api/work-items", {
    title: `titled item ${stamp}`,
    content: "the tab should name this by id",
    project_id: pid,
  });
  const proposal = await post("/api/proposals", {
    title: `titled proposal ${stamp}`,
    summary: "s",
    project_id: pid,
    work_item_numbers: [wi.wi_number],
  });
  const card = await post("/api/cards", { title: `titled card ${stamp}` });
  const link = await post("/api/links", {
    url: `https://example.invalid/title-${stamp}`,
    title: `titled link ${stamp}`,
  });
  const schedule = await post("/api/schedules", {
    title: `titled schedule ${stamp}`,
    cadence: "monthly",
  });
  const handoff = await post("/api/handoffs", {
    title: `titled handoff ${stamp}`,
    summary: "s",
    body: "context",
    related_node_ids: [wi.node_id],
  });
  const program = await post("/api/programs", {
    title: `titled program ${stamp}`,
    aim: "the aim",
    slices: [proposal.node_id],
  });

  const upload = await request.post("/api/img", {
    multipart: {
      file: { name: "title.png", mimeType: "image/png", buffer: PNG },
    },
  });
  expect(upload.ok(), "the image fixture should upload").toBeTruthy();
  const attachment = await upload.json();

  // Four call sites, eight of the nine kinds. `NodeDetail` backs six pages with
  // one expression, so a sample of it proves that site; the other three have a
  // `<svelte:head>` each and are all here.
  const cases: [string, number, string][] = [
    ["workitem", wi.wi_number, `/work-items/${wi.wi_number}`],
    ["card", card.node_id, `/cards/${card.node_id}`],
    ["link", link.node_id, `/reading-list/${link.node_id}`],
    ["schedule", schedule.node_id, `/schedules/${schedule.node_id}`],
    ["attachment", attachment.node_id, `/attachments/${attachment.node_id}`],
    ["sprint_proposal", proposal.node_id, `/planning/${proposal.node_id}`],
    ["handoff", handoff.node_id, `/handoffs/${handoff.node_id}`],
    ["program", program.node_id, `/programs/${program.node_id}`],
  ];

  for (const [kind, id, path] of cases) {
    await page.goto(path);
    await expect(page, path).toHaveTitle(docTitle(kind, id));
  }

  // The attachment is the one kind whose tab id is not the number in its own
  // URL: it says `img-7ac`, the spelling the markdown token and
  // `get_attachment` use. `imgIdFromNodeId` derives that client-side from the
  // node id, so assert it against the id the *server* minted — the two are the
  // same number in hex and this is what proves it.
  await page.goto(`/attachments/${attachment.node_id}`);
  await expect(page).toHaveTitle(`korg — attachment ${attachment.img_id}`);

  // `report` is the ninth kind and has no POST route — reports are written by
  // agents through MCP — so it is covered by the korg-core fence rather than
  // here. Assert that this list is otherwise complete, so a tenth kind shows up
  // as a failure here too rather than as an untested page.
  const covered = new Set(cases.map(([kind]) => kind));
  covered.add("report");
  expect([...NODE_KINDS].filter((k) => !covered.has(k))).toEqual([]);
});

test("a page that is not a node detail keeps the plain title", async ({
  page,
}) => {
  // Ken's "ideally it goes back to `korg` once I'm out of a detail". Most list
  // pages set no title at all and get `app.html`'s; the ones that do set one
  // now all read the same way, `Search · korg` included (#1966).
  await page.goto("/work-items");
  await expect(page).toHaveTitle("korg");
  await page.goto("/search");
  await expect(page).toHaveTitle("korg — search");
});
