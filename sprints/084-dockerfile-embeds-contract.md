# 084 — `contract/` is a build input, and the gate now knows it

No proposal. A same-day fix for a defect sprint 083 shipped, found by 083's own
deploy. Run as part of leg `korg-328c86`'s ship turn on kai.

## What happened

Sprint 083 added `include_str!("../../../contract/read-shapes.json")` to
`korg-api`, so the published read-shape contract is embedded in the binary and
served at `GET /api/contract/read-shapes`. That is the right design — a consumer
needs to know what the korg it is talking to promises, and the embedded file is
exactly what the running binary was built from.

The image build then failed:

```
error: couldn't read `crates/korg-api/src/../../../contract/read-shapes.json`:
       No such file or directory (os error 2)
```

The Dockerfile's `rust` stage copies `Cargo.toml`, `Cargo.lock`,
`rust-toolchain.toml` and `crates/`. Nothing else. `contract/` is a new
top-level directory, so it was never copied, and the release binary could not
compile.

## Why no gate caught it

This is the part worth keeping, because the fix is trivial and the gap is not.

`just check` and CI both compile **in the full tree**, where the file is
obviously present. The only build with a restricted view of the repo is the
image's `rust` stage, and nothing in the repo related the two. So the defect was
invisible to every gate by construction, and surfaced at the worst possible
point: after the merge, after CI was green, and after a multi-minute build.

083's own gates were thorough about the *document* — six tests, each perturbed
to prove it could fail — and none of them could have known the file would not be
present at image-build time. Test coverage of an artefact's content says nothing
about whether the build can see it.

## What shipped

- **`COPY contract/ ./contract/`** in the rust stage, with a comment stating the
  file is a build input rather than a repo artefact. That sentence is the thing
  that was not obvious; the line is a one-liner.
- **`embedded_files_are_copied_into_the_image`** in `crates/korg-mcp/tests/docs_drift.rs`.
  It resolves every `include_str!`/`include_bytes!` literal in the workspace
  crates to a repo-relative path and asserts the rust stage copies its
  top-level directory. Paths under `crates/` are skipped — they ride along with
  the crate sources.

`docs_drift` is the right home: the suite already exists to assert that things
outside the compiler's view agree with the code, which is precisely this.

**Checked at the granularity `COPY` works at**, which is also the granularity the
failure had — `crates/` was copied and `contract/` was not. Resolving to the
top-level directory rather than the exact path is deliberate: a per-file check
would pass a `COPY contract/read-shapes.json` that a later second file in the
same directory would silently defeat.

## Verified by making it fail

Removing the `COPY` line again fails the guard, naming the including file, the
embedded path, and the remedy:

```
crates/korg-api/src/lib.rs embeds contract/read-shapes.json (../../../contract/read-shapes.json)
Add a `COPY <dir>/ ./<dir>/` line to the rust stage.
```

`just check` exit 0, 82 suites. `docs_drift` 19 → 20.

## Repaired in passing

Nothing else. This sprint *is* the repair.

## Deployed

**2026-09-18, to kubsdb**, in the same image as sprint 083 — `1404555d02b5`,
which *is* this sprint's merge commit. 083 could not deploy without it, so there
was never a build carrying one and not the other.

Verification is recorded in full on
[083's record](083-published-read-contract.md#deployed): the in-deploy revision
assertion, `post-deploy-check.sh --compare` green with every row count
unchanged, and `GET /api/contract/read-shapes` serving the committed document
byte-for-byte from the deployed release — which is the endpoint whose build this
sprint unblocked.

The proof specific to *this* sprint is simply that the image built at all. The
previous attempt, from 083's own commit, did not.

## Note on sequencing, since this record was written twice

The first version of this file was committed mid-flight and immediately failed
`every_shipped_sprint_since_047_records_its_deploy`. That gate exempts only the
highest-numbered record — the sprint in flight, whose deploy happens after the
merge — so introducing 084 turned 083 into a *shipped* sprint that owed a
`## Deployed` section it could not yet have.

The gate was right and the record was premature. It was withdrawn (`2d77c08`)
rather than satisfied with a `## Not deployed` heading that would have been
false, and it lands here with the deploy result that now actually exists.
