# 048 — korg deploys via the registry

**Proposal:** korg:1021 · **Work items:** #1011 (chore, S)
**Branch:** `048-registry-deploy`
**Program:** korg:1026 "Deploy from the store — fleet adoption" — slice 1 of 3.
hv-simulator #1018 and apt-temps #1019 follow, and this sprint's shape is meant
to be the thing they copy.

## Goal

Replace `docker save | ssh kubsdb docker load` with a push/pull against the
homelab docker registry (`kubsdb.encke-wahoo.ts.net:5000`, live since k-homelab
sprint 020), tagging real versions so the registry's history *is* the rollback
path.

korg goes first because it deploys most often of anything in the homelab, so it
is where image history and one-tag rollback pay off soonest — and where a
mistake surfaces fastest.

## The transport was the only thing wrong

Sprint 047 had just made the run config a committed, authoritative file and
landed the first compose-driven deploy. So this sprint had a genuinely small
target: **only the transport**. Build, label, verify, rollback semantics and the
compose file's authority all stay exactly as 047 left them. The diff is `save |
ssh | load` → `push` / `pull`, plus the doc changes that keeps honest.

Resisting scope creep here was the main design decision. It would have been easy
to also fold in a `just deploy` recipe or restructure the skill; both would have
made the template that #1018 and #1019 copy harder to read.

## Verified before it was written

The registry round trip was exercised *first*, against the real production
image, and the skill was written from what actually happened rather than from
what the docs implied:

- `docker push` from kai — anonymous, TLS from `tailscale serve`, no
  `insecure-registries` entry, no login. The catalog went from `[]` to
  `["korg"]`; korg is the registry's first repository.
- `docker pull` on kubsdb from `kubsdb.encke-wahoo.ts.net:5000` — kubsdb
  reaching *itself* over the tailnet — returned image id `39bbc8f856d5`,
  identical to the local one.

- The step-3 revision gate, both branches, against the live container:
  matching revision exits 0, `deadbeefdeadbeef` prints `REVISION MISMATCH` and
  exits 1. This also confirmed `ssh kubsdb bash -s "$REV"` passes `$1` intact
  through kubsdb's **fish** login shell — the `{{index …}}` template stays
  inside the heredoc, where fish never sees it.
- `docker compose config` resolves the new `image:` both ways: default
  `…:5000/korg:latest`, and `…:5000/korg:<sha>` under `KORG_IMAGE`.

Both pushes seeded the registry with sprint 047's image as `korg:1eacecd10af7`
**and** `korg:latest`, so the registry's `latest` matches what is actually
deployed and a registry-resident rollback target existed before the first
registry deploy rather than one deploy later.

## Decisions

- **The tag is the 12-char git short SHA**, matching the
  `org.opencontainers.image.revision` label the deploy already stamps.
  Considered and rejected: a semver tag. korg's crates are all a flat `0.1.0`
  and are not bumped per release, so `korg:0.1.0` would be overwritten by every
  build and name nothing — the exact no-history outcome the proposal warns
  `latest`-only would produce. The commit is korg's real version, and every
  sprint record already cites it as the rollback target.
- **Push both `<short-sha>` and `latest`, SHA first.** Both halves are the same
  lesson as WI #584 (which fixed `docker save` shipping only `latest`, leaving
  the previous image on kubsdb as `<none>`). Ordering matters on failure: if
  the second push dies, the registry holds a complete named build and `latest`
  still points at the last good one. The reverse order publishes a `latest`
  with no durable name.
- **The compose file names the tailnet address, not `localhost:5000`.** The
  registry listens on `127.0.0.1:5000` on kubsdb, so `localhost` would work
  *there* — but the image reference is one committed string used by the host
  that pushes and the host that pulls, and those must be the same name. The
  cost is explicit in the skill: a deploy now needs tailscaled up on kubsdb
  even though the registry is running locally on it.
- **`docker compose pull` is required — and the deploy asserts the revision
  rather than trusting the pull's output.** Compose does not re-fetch an image
  it already has cached under that name, so without the pull `up -d` would
  silently re-run whatever `latest` kubsdb fetched last time — a quieter
  version of the false pass the old skill flagged for `docker load`, because
  there is no missing transfer to notice.

  The first draft of this sprint told the operator to read `Downloaded newer
  image` vs `Image is up to date` from the pull. **That was wrong**, and
  running it is what caught it: `docker compose pull` prints `Pulling` /
  `Pulled` identically whether it downloaded anything or matched a cache. Those
  strings are `docker pull`'s, not compose's. So step 3 now ends by comparing
  the running container's `org.opencontainers.image.revision` label against the
  commit the run built, and exits non-zero on a mismatch — a gate that catches
  a no-op pull, an unmoved `latest`, and an adopted stale container at once,
  and that depends on no command's phrasing. `set -euo pipefail` on the remote
  script keeps a failed pull from falling through to `up -d`.

  Worth stating as a general lesson for #1018/#1019: an instruction to *read
  output* is only as good as the output, and this one was never verified
  against the command it described.

## Shipped

- `deploy/docker-compose.yml` — `image:` now defaults to
  `kubsdb.encke-wahoo.ts.net:5000/korg:latest`; `KORG_IMAGE` still overrides it
  for rollback, now against a registry tag.
- `.claude/skills/deploy-kubsdb/SKILL.md` — new **The registry** section (what
  it is, why the tailnet name, why both tags); preflight gains a registry
  health check and a "is the rollback target actually pushed?" check; build
  tags registry-qualified names directly; ship becomes two pushes; deploy gains
  `docker compose pull`; **Rollback** rewritten around the tags list.
- `docs/operations.md` — deployment table's Image row; **Rollback** rewritten
  (it still described the pre-047 `docker run`, which was stale on two counts);
  **Cold start** gains the pull and the new dependency note.

Updating the skill in the same change is the point, not tidiness: a skill that
still names the old path is a durable bug, and this repo has already paid for
that once — sprint 009's skill encoded a `docker run` approach that sprint 004
had explicitly recorded as wrong, and every deploy for 38 sprints inherited it.

## What cold start gained, unplanned

The image now being a registry reference closes a hole in the cold-start path
that neither 047 nor this proposal noticed. `deploy/cold-start.sh` creates the
role, the database and `korg.env` — but on a rebuilt host there was no korg
image either, and nothing in the procedure said so; `docker compose up -d`
would have failed on a missing local image. A registry-qualified image pulls.
Recorded in `docs/operations.md` rather than left as a nice surprise.

The matching new dependency is recorded there too: korg's deploy needs the
`registry` container up, and a kubsdb loss that takes `/datastore` takes out
both, so k-homelab restores the package store before korg. `/datastore/packages`
is mirrored to the NAS nightly.

## Follow-ups, filed not implied

- **Supersede the klams korg memory at deploy time, not before.**
  `019f541f-7752-7162-aa15-a883147bb90f` records *"Deploys: build `korg:latest`
  locally, `docker save | ssh kubsdb docker load` … No image registry."* That
  is still accurate for the **running** deployment — kubsdb's
  `/datastore/korg/docker-compose.yml` is the pre-cutover copy until this
  branch deploys — so it was deliberately left alone rather than updated to
  describe a state that is not yet live. Supersede it once the deploy lands,
  in the same sitting as the sprint's Deployed section.

- **kwebi has its own `deploy-kubsdb` skill** (`~/src/tools/kwebi`) which also
  states there is no registry. It is not in the #1026 program, which names only
  hv-simulator #1018 and apt-temps #1019. Worth adding as a fourth slice.
- Pre-cutover korg images are not in the registry — only builds pushed from
  this sprint onward are. The old ones remain on kubsdb under their unqualified
  `korg:<short-sha>` names and still work as `KORG_IMAGE` values; the skill and
  operations.md both say where to look for each. Backfilling them was not
  attempted: they are a shrinking set with a working home.
