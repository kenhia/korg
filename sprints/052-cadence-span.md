# 052 — Cadence inference gates on history span, not report count

Proposal korg:1098 (rank 1). Covers #1097 (S). Branch `052-cadence-span`, run
2026-08-08 — hours after 051 shipped the feature it fixes.

## What happened

Sprint 051 deployed staleness age-out. Against production it raised exactly one
alert:

> **kyac — stale, 21 days overdue.**

Ken: kyac is **interactive**. It files a report when he prompts it; there is no
schedule behind it. Its silence carries no information at all.

korg had inferred a 2-day cadence from this:

```
2026-07-09, 07-11, 07-12, 07-14     gaps: 2, 1, 2
```

`SOURCE_MIN_HISTORY = 3` gates on the **count** of reports. kyac cleared it with
four, and korg concluded "files every couple of days, therefore three weeks
late".

## Why the obvious fix is the wrong fix

The first instinct is a variance or dispersion check — refuse a cadence when the
gaps are irregular. **It would have done nothing here.** Gaps of 2, 1, 2 are
about as regular as a series gets.

That is not a weakness in the check. It is a fact about the data: a healthy
episodic source prompted a few times in one working week and a broken daily
source produce *the same shape*. **History cannot tell you a source is
scheduled** — the information is not in it.

Worth stating plainly because the next person to look at this will reach for
statistics first, and the data will look like it should work.

## What does separate them

Span, not regularity. Measured live on 2026-08-08:

| source | n | span | inferred cadence | span ÷ cadence |
|---|---|---|---|---|
| `kmon` | 23 | 34d | 1 | **34×** |
| `kyac` | 4 | 5d | 2 | **2.5×** |

`SOURCE_MIN_HISTORY` measures the wrong quantity. What makes an inferred cadence
believable is having watched it **repeat over time**, not having collected
several samples of it in one afternoon.

So: `SOURCE_MIN_SPAN_CADENCES = 7`. The observed history must span at least
seven of the cadence it claims before korg believes it. 7 sits well clear of
both ends rather than just barely excluding kyac — a threshold tuned to one data
point is one that will be wrong on the next.

## The result

kyac becomes **`unrated`**, which was already the right word: *korg cannot judge
this source.* Not retired (it has not ended), not broken, not fresh. No new
vocabulary, no new column, no migration — the fix corrects what "enough history"
means.

`history_span_days` joins `SourceHealth` so `unrated` is explicable rather than
mysterious, and the Reports strip now says "4 reports over 5 days — no cadence
korg will believe yet" instead of a bare shrug.

## Correcting 051's follow-up

The 051 record suggested `set_report_source("kyac", retired: true)` as the
answer if kyac had been deliberately stopped. **Do not do that.** `retired`
means *this source has ended*; kyac has not. A source silenced with the wrong
meaning becomes a landmine the day it gains a real schedule, and a silenced
source nobody remembers to un-silence is #950's failure wearing a different hat.
The span rule reaches the right state with nobody declaring anything, and if
kyac later starts keeping a schedule it is rated on it automatically — there is
no marker to remember to clear. `an_episodic_source_reaches_the_right_state_
without_being_retired` pins both halves.

## Decisions

- **D-1** — Gate inference on span, not variance. Regularity is exactly what a
  burst has.
- **D-2** — Threshold 7 cadences, chosen for clearance from both real cases, not
  fitted to kyac.
- **D-3** — Keep `SOURCE_MIN_HISTORY`. The two gates answer different questions:
  "is there any gap to measure?" and "has the pattern held?".
  `two_distant_reports_are_still_not_a_cadence` fences that they are not
  redundant.
- **D-4** — A **declared** cadence bypasses the span gate entirely. Declaring
  one is a statement that a cadence is *expected* — the thing inference can
  never establish — so it is authoritative from day one. This is also the escape
  hatch for a real scheduled source too young to have earned one.
- **D-5** — No `on_demand` declaration, yet. `unrated` already says the true
  thing for an episodic source, and it costs nobody a write. If episodic sources
  become common, an explicit declaration beats a cleverer inference — but adding
  vocabulary for a single case is how vocabularies rot.

## The residue, named

korg:1079 recommended "inferred with a declared override"; 051 implemented that
faithfully, and it is still the right default. But inference can only suggest
*what* a cadence is — it cannot establish that one is *expected*.

Requiring a declaration before any alerting would close that completely and
reintroduce #950's original failure: a genuinely new scheduled source nobody
declared, silently unwatched. The span gate is the trade — keep the useful
default, remove the class of guess that produced this alert. That trade is a
choice, not a proof, and it is written down here so the next person changing it
knows which way it was made and why.

## 051 fixtures this changed

Three sprint-051 tests used a five- or six-day run as shorthand for "a daily
source". The new rule correctly refuses to read those as cadences, so the
fixtures were lengthened to mean what they intended, with a comment at each site
saying why. The load-bearing #950 assertions — the July replay and the
retirement test — used fourteen-report histories and **passed untouched**, which
is the reassuring part: the fix did not need the tests that matter to move.

## Verified

`just check` green. `sprint052.rs` is six cases, including the two that matter
most in opposite directions: `a_regular_burst_is_not_a_cadence` (kyac's timeline
verbatim) and `a_long_running_daily_source_that_stops_is_still_the_alert` — a
month of daily filing then eleven days of silence, proving the span gate did not
buy its false-positive fix by going quiet.

## Deployed

2026-08-08, to kubsdb. Image `korg:4d9d45a7d1ac` (also `latest`), revision gate
asserted. Rollback target: `korg:82b81e621fe5` (sprint 051) — a clean re-tag,
since 052 carries no migration.

Migrations unchanged at 25. Row counts flat across the deploy (work_items 751,
cards 30, links 4, proposals 171, projects 40); `node_count` 1000 → 1001 and the
sequence 1099 → 1100, from another session writing to korg while the image
built — explainable, and upward.

**The fix, verified live:**

| source | freshness | n | span | cadence | alerts |
|---|---|---|---|---|---|
| `kmon` | fresh | 23 | 34d | 1 | no |
| `kyac` | **unrated** | 4 | 5d | — | **no** |
| 5 one-off probes | unrated | 1 | 0d | — | no |

kyac is no longer an alert and kmon is untouched, which is exactly the pair this
sprint had to get right. korg's staleness panel now shows **zero alerts** — the
correct state, because nothing is actually broken.

### Noticed during verification

`scripts/post-deploy-check.sh` counts reports with `curl /api/reports | jq
length`, and `/api/reports` defaults to `limit=30`. The count therefore
**saturates at 30** and cannot detect loss above it — a count that cannot go
down cannot do the one job the compare exists for. Two new sources appeared in
this deploy's source list while `reports` sat at 30, which is what exposed it.
Filed separately; the same latent issue may affect `proposals` (171 today, so
not yet capped, but it is a bare-array read too).
