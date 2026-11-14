#!/usr/bin/env python3
"""korg-calendar: publish Ken's tagged days and the holidays as `kdash:calendar`.

korg WI 4664, sprint 101, slice 2 of program korg:4669 (kpidash's time and
calendar card). The contract is kdashdata's `kdash-calendar` schema (CD-30),
pinned in `calfeed/vendor/` at kdashdata b1ad6af.

A korg *reader*, like korg-pulse: it reads `GET /api/tagged-days`, computes US
federal holidays by rule (`us_holidays.py`), merges them into one entry per date,
and writes with `kdash-pub`. The korg service gains no Redis dependency
(kdashdata CD-26). Stdlib only, so the unit runs it straight from the checkout.

**Hourly, and on change.** korg runs on kubsdb and the publisher on kai, so a
tag added through MCP cannot poke the publisher. Instead the timer fires every
minute and this script decides: it builds the record, reads the live one back,
and writes only when the window or the days differ, or when the live record is
nearly an hour old. A change therefore shows within a minute, an unchanged
calendar is rewritten once an hour (`expected_refresh_s` 3600), and a quiet
minute writes nothing and logs nothing.

**Dates are America/Los_Angeles**, never the hosts' UTC clock: `today`, the
first of the month, the yearly expansion and the holiday dates are all local.
`off` means a day Ken tagged off and nothing else — a holiday he did not tag is
`off: false`.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import pathlib
import subprocess
import sys
import time
import urllib.request
from typing import Any, Iterable
from zoneinfo import ZoneInfo

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from us_holidays import holidays_between  # noqa: E402

KEY = "kdash:calendar"
TZ = ZoneInfo("America/Los_Angeles")
#: Hourly. The timer fires every minute (systemd/korg-calendar.timer) but only
#: rewrites an unchanged record once it is this old, less `REFRESH_SLACK_S`.
DEFAULT_REFRESH_S = 3600
REFRESH_SLACK_S = 120
#: The contract asks for at least 90 days past `ts`'s date. Two spare days keep
#: that true for a reader that dates `ts` in UTC, which can be a day ahead.
AHEAD_DAYS = 92
MAX_DAYS = 64
NAME_MAX = 31
SHORT_MAX = 12
JOIN = " / "
DEFAULT_KORG_URL = "https://kubsdb.encke-wahoo.ts.net:5674"


def local_today(ts: float) -> dt.date:
    return dt.datetime.fromtimestamp(ts, TZ).date()


def window(today: dt.date) -> tuple[dt.date, dt.date]:
    return today.replace(day=1), today + dt.timedelta(days=AHEAD_DAYS)


def on_year(d: dt.date, year: int) -> dt.date:
    """`d`'s month and day in `year`; 29 February falls back to the 28th."""
    try:
        return d.replace(year=year)
    except ValueError:
        return d.replace(year=year, day=28)


def expand(tags: Iterable[dict], start: dt.date, end: dt.date) -> list[tuple[dt.date, dict]]:
    """Each stored tag at every concrete date it lands on in [start, end].

    A yearly tag repeats from its own year on, never before it.
    """
    out = []
    for tag in tags:
        d = dt.date.fromisoformat(tag["date"])
        if tag.get("yearly"):
            dates = [on_year(d, y) for y in range(max(start.year, d.year), end.year + 1)]
        else:
            dates = [d]
        out.extend((x, tag) for x in dates if start <= x <= end)
    return out


def joined(names: list[str]) -> str:
    """Two names on one date share the one string, cut to fit the contract."""
    s = JOIN.join(dict.fromkeys(names))
    return s if len(s) <= NAME_MAX else s[: NAME_MAX - 1] + "…"


def build_record(tags: list[dict], ts: float, refresh_s: int = DEFAULT_REFRESH_S) -> dict:
    """The `kdash:calendar` record for these stored tags at `ts`.

    Pure: no I/O and no clock beyond `ts`, so a fixture pins the whole
    contract.
    """
    start, end = window(local_today(ts))
    by_date: dict[dt.date, dict[str, Any]] = {}

    def slot(d: dt.date) -> dict[str, Any]:
        return by_date.setdefault(d, {"off": False, "holidays": [], "specials": []})

    for h in holidays_between(start, end):
        slot(h.date)["holidays"].append(h)
    for d, tag in expand(tags, start, end):
        if tag["kind"] == "off":
            slot(d)["off"] = True
        elif tag["kind"] == "special":
            slot(d)["specials"].append(tag)

    days = []
    for d in sorted(by_date)[:MAX_DAYS]:
        s = by_date[d]
        entry: dict[str, Any] = {"date": d.isoformat(), "off": s["off"]}
        if s["holidays"]:
            entry["holiday"] = joined([h.name for h in s["holidays"]])
        if s["specials"]:
            entry["special"] = joined([t["label"] for t in s["specials"]])
        # The special day's stored short wins; a holiday's is the fallback; a
        # day that is only off has none and the card picks its own.
        short = next((t["short"] for t in s["specials"] if t.get("short")), None)
        if short is None and s["holidays"]:
            short = s["holidays"][0].short
        if short:
            entry["short"] = short[:SHORT_MAX]
        days.append(entry)

    return {
        "ts": ts,
        "expected_refresh_s": refresh_s,
        "from": start.isoformat(),
        "to": end.isoformat(),
        "days": days,
    }


def should_publish(record: dict, live: dict | None, now: float) -> str | None:
    """Why to write `record` over `live` now, or None to leave it be."""
    if live is None:
        return "absent"
    same = all(live.get(k) == record[k] for k in ("from", "to", "days", "expected_refresh_s"))
    if not same:
        return "changed"
    try:
        age = now - float(live["ts"])
    except (KeyError, TypeError, ValueError):
        return "changed"
    if age >= record["expected_refresh_s"] - REFRESH_SLACK_S:
        return "refresh"
    return None


def read_tags(base: str) -> list[dict]:
    with urllib.request.urlopen(base.rstrip("/") + "/api/tagged-days", timeout=30) as resp:
        return json.load(resp)


def read_live(kdash_pub: str) -> dict | None:
    """The record central holds now; None when it is unset or unreadable.

    Unreadable is treated as absent on purpose: the write that follows fails
    loudly if central is really down, and succeeds if only the read was odd.
    """
    r = subprocess.run([kdash_pub, "--app", "korg-calendar", "get", KEY],
                       capture_output=True, text=True)
    if r.returncode != 0:
        return None
    try:
        live = json.loads(r.stdout)
    except json.JSONDecodeError:
        return None
    return live if isinstance(live, dict) else None


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="korg-calendar", description=__doc__.splitlines()[0])
    ap.add_argument("--korg", default=os.environ.get("KORG_URL", DEFAULT_KORG_URL),
                    help=f"korg base URL (env KORG_URL; default {DEFAULT_KORG_URL})")
    ap.add_argument("--kdash-pub", default="kdash-pub", help="kdash-pub binary (default: on PATH)")
    ap.add_argument("--dry-run", action="store_true", help="print the record; write nothing")
    ap.add_argument("--force", action="store_true", help="write even if nothing changed")
    args = ap.parse_args(argv)

    now = time.time()
    record = build_record(read_tags(args.korg), now)
    if args.dry_run:
        print(json.dumps(record, indent=2, ensure_ascii=False))
        return 0

    why = "forced" if args.force else should_publish(record, read_live(args.kdash_pub), now)
    if why is None:
        return 0
    payload = json.dumps(record, separators=(",", ":"), ensure_ascii=False)
    subprocess.run([args.kdash_pub, "--app", "korg-calendar", "set", KEY, payload], check=True)
    print(f"korg-calendar: {KEY} ({why}) from={record['from']} to={record['to']} "
          f"days={len(record['days'])} off={sum(d['off'] for d in record['days'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
