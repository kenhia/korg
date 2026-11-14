#!/usr/bin/env python3
"""korg-pulse: publish korg's board numbers as `kdash:korg:pulse`.

korg WI 4611, sprint 100, slice 3 of program korg:4619 (kpidash row 2). The
contract is kdashdata's `kdash-korg-pulse` schema (CD-28), pinned in
`pulse/vendor/` at kdashdata d57e1dc.

A korg *reader*, not part of korg: it reads two public REST rollups and writes
with `kdash-pub`, so the korg service gains no Redis dependency (kdashdata
CD-26). Stdlib only, so the systemd unit can run it straight from the checkout
with nothing built or installed.

Every figure is COPIED from korg, never re-derived (CD-26):

- `open` and `history` come from `/api/work-items/flow` — today's `backlog`
  and that column day by day. So the headline and the sparkline's last point
  cannot disagree. korg reconstructs the series from its transition log, so
  the publisher stores nothing between runs.
- `by_project`, `projects`, `awaiting`, `active` and `queued` come from
  `/api/board` — `depth[].wi_total`, `awaiting`, `active`, and the `proposed`
  rows of `queue`. Parked rows ride at the end of `queue` and are not counted.

`other_open` is the one figure that combines the two reads: `open` minus what
`by_project` lists, so it holds the cap's overflow plus unassigned items. The
reads are two requests, so a write landing between them can make the
per-project sum exceed `open`. That record would break the sum rule, so it is
re-read once, then refused rather than published.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.request
from typing import Any

KEY = "kdash:korg:pulse"
BY_PROJECT_CAP = 24
HISTORY_CAP = 30
#: The timer's cadence (systemd/korg-pulse.timer). The reader greys the card
#: once `ts` is older than this plus its own grace, so change the two together.
DEFAULT_REFRESH_S = 300
DEFAULT_KORG_URL = "https://kubsdb.encke-wahoo.ts.net:5674"


class Inconsistent(Exception):
    """The board and the flow disagree beyond what `other_open` can absorb."""


def build_record(board: dict, flow: dict, ts: float, refresh_s: int) -> dict:
    """The pulse record for one board read and one flow read.

    Pure: no I/O and no clock, so a fixture pair pins the whole contract.
    """
    days = flow.get("days") or []
    if not days:
        # The flow always ends today, so an empty series means korg answered
        # something other than the flow, and there is no `open` to copy.
        raise Inconsistent("work_item_flow returned no days; nothing to copy `open` from")
    open_now = int(days[-1]["backlog"])

    history = [{"date": d["day"], "open": int(d["backlog"])} for d in days][-HISTORY_CAP:]

    holding = [(r["project"], int(r["wi_total"])) for r in board["depth"] if int(r["wi_total"]) > 0]
    holding.sort(key=lambda p: (-p[1], p[0]))
    listed = holding[:BY_PROJECT_CAP]

    if sum(n for _, n in holding) > open_now:
        raise Inconsistent(
            f"projects hold {sum(n for _, n in holding)} open items but the backlog is {open_now}"
        )

    return {
        "ts": ts,
        "expected_refresh_s": refresh_s,
        "open": open_now,
        "projects": len(holding),
        "awaiting": len(board["awaiting"]),
        "active": sum(1 for p in board["active"] if p.get("status") == "active"),
        "queued": sum(1 for p in board["queue"] if p.get("status") == "proposed"),
        "by_project": [{"name": name, "open": n} for name, n in listed],
        "other_open": open_now - sum(n for _, n in listed),
        "history": history,
    }


def _get(base: str, path: str) -> Any:
    with urllib.request.urlopen(base.rstrip("/") + path, timeout=30) as resp:
        return json.load(resp)


def read_korg(base: str) -> tuple[dict, dict]:
    return _get(base, "/api/board"), _get(base, f"/api/work-items/flow?days={HISTORY_CAP}")


def snapshot(base: str, refresh_s: int) -> dict:
    """Read korg and build the record, re-reading once on a torn pair."""
    for attempt in (1, 2):
        board, flow = read_korg(base)
        try:
            return build_record(board, flow, time.time(), refresh_s)
        except Inconsistent:
            if attempt == 2:
                raise
    raise AssertionError("unreachable")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="korg-pulse", description=__doc__.splitlines()[0])
    ap.add_argument("--korg", default=os.environ.get("KORG_URL", DEFAULT_KORG_URL),
                    help=f"korg base URL (env KORG_URL; default {DEFAULT_KORG_URL})")
    ap.add_argument("--refresh-s", type=int, default=DEFAULT_REFRESH_S,
                    help="expected_refresh_s to declare; match the timer (default %(default)s)")
    ap.add_argument("--kdash-pub", default="kdash-pub", help="kdash-pub binary (default: on PATH)")
    ap.add_argument("--dry-run", action="store_true", help="print the record; write nothing")
    args = ap.parse_args(argv)

    try:
        record = snapshot(args.korg, args.refresh_s)
    except Inconsistent as exc:
        print(f"korg-pulse: refusing to publish: {exc}", file=sys.stderr)
        return 1
    payload = json.dumps(record, separators=(",", ":"))

    if args.dry_run:
        print(json.dumps(record, indent=2))
        return 0
    subprocess.run([args.kdash_pub, "--app", "korg-pulse", "set", KEY, payload], check=True)
    print(f"korg-pulse: {KEY} open={record['open']} projects={record['projects']} "
          f"awaiting={record['awaiting']} active={record['active']} queued={record['queued']} "
          f"history={len(record['history'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
