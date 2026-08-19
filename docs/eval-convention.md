# Evaluating an agent against korg

**Hand this page to any agent doing evaluation work, and read it before
writing a harness that points at korg.** It exists so a run's residue is
findable and disposable afterwards, which it is not by default: korg is
no-auth HTTP, so the server cannot tell an eval's writes from real ones.
Nothing here is enforced. It is a convention, and it is the whole mechanism.

## The rule

> **Everything an evaluation creates goes in the `eval` project.**

Work items, cards, links, proposals, reports — every write that takes a
project takes `project: "eval"`. That is the entire convention.

```jsonc
// MCP
{"title": "…", "project": "eval", "wi_type": "task"}
```

```bash
# REST
curl -X POST "$KORG/api/work-items" \
  -H 'content-type: application/json' \
  -d '{"title":"…","project":"eval"}'
```

For a node kind that takes no project — a `program` spans projects by
design, and an `area` belongs to one — either skip it in the eval or clean
it up by hand afterwards. There is no third option, and there is deliberately
no `delete_project`.

## Why the project and not something cleverer

`eval` carries `category: "EVAL"`, a closed-vocabulary value that has existed
since migration 0018 and already groups harness leakage — `loglens` and
`feedhub` were mapped to it there. korg's disposal doctrine
([api.md](api.md#disposal-semantics-wi-855)) states the principle directly: *test residue is a
`category`, not a disposal*.

Three alternatives were considered on #466 and rejected:

- **A per-item `tags: ["eval"]` convention.** Same enforcement (none), but the
  agent has to remember it on every single call rather than once. A project is
  one name a harness is told at setup.
- **A first-class `origin` field on `node`.** A schema change plus every
  surface, to buy what a project name already buys.
- **A scratch korg instance.** Costed and rejected on 2026-08-15. It is the
  strongest isolation and the wrong trade at Ken's actual eval volume: it adds
  friction to *every* run — start the instance, repoint the MCP config — and
  if either step is forgotten the run writes to production anyway, silently.
  So it does not remove the discipline requirement, it adds one, plus a compose
  file and a runbook to maintain. **Do not re-propose it without new volume
  evidence**; the full costing is in #466's comments and in the `eval`
  project's `notes`.

The containment argument that would have favoured a separate instance — an
eval agent going off-script — is weak here, because korg's blast radius is
already bounded: a hard delete refuses when a row is referenced, and most
kinds have no delete at all.

## The `eval` project must stay `active`

Do not archive it while evaluations are still expected.

An earlier sketch of this convention assumed the eval project would sit at
`inactive`/`archived` so the UI's project rail would hide it. That is not
possible: since #884, both `project_id_for_name` and `require_project` refuse
an archived project as a write target, on the name path *and* the id path —

```
project 'eval' is archived and cannot take new work
```

— so an archived eval project cannot receive the items it exists to hold.
Containment comes from the `EVAL` category and the project's description,
which tells any agent reading the routing roster that real work does not
belong there.

That makes eval residue visible to every project-spanning read, which is why
`category` now rides the project rollup as well as `ProjectRow` (#1414, sprint
068): `get_board`'s `depth` and `GET /api/proposals/rollup` carry it, so a
consumer drops EVAL rows from the one read it was already making. korg does
**not** filter them for you and ships no `is_eval` flag — an EVAL project is
the tier below every tier, and the project-tiers design (korg+ plan GP-10) is
meant to absorb this exclusion rather than have a parallel switch grow beside
it.

## Cleaning up after a run

By hand, and quickly — that is the trade this convention accepted:

1. `list_work_items` with `project: "eval"` (add `wi_status: "all"`), and the
   same for any other kind the run touched.
2. Archive what you do not want. Archiving is lossless and reversible;
   `EVAL` keeps the residue findable as a group either way.
3. Leave the project itself alone unless you are certain no further evals are
   coming. If you do archive it, **re-activate that same project** for the
   next run rather than creating a second eval project — two of them is the
   drift this page exists to prevent.

## Related conventions

A **report source** written by a harness probe is the same problem in a
different store, and it has its own rule: retire the source in the same
session that writes it, with `set_report_source(source, retired: true)` and a
note saying what it was. Six one-off probe sources accumulated on the Daily
Reports page before anyone noticed (#1398).
