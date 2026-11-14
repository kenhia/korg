"""US federal holidays, computed by rule for any year (korg WI 4664).

Every federal holiday (5 U.S.C. 6103(a)) is either a fixed date or the nth
weekday of a month, so there is no data file to refresh each January. A
fixed-date holiday falling on a weekend is ALSO observed on a weekday
(Saturday -> the Friday before, Sunday -> the Monday after), and the feed lists
that weekday as its own holiday named `<Name> (observed)` (kdashdata CD-30).

Names are the common ones that fit the feed's 31 characters ("Juneteenth",
not the statutory long form), and each has a `short` of at most 12.
"""

from __future__ import annotations

import datetime as dt
from typing import NamedTuple

MON, THU, SAT, SUN = 0, 3, 5, 6


class Holiday(NamedTuple):
    date: dt.date
    name: str
    short: str


# (month, day, name, short, observed short) — the fixed-date holidays, the only
# ones that can fall on a weekend. Juneteenth starts in 2021 (Pub. L. 117-17).
FIXED = [
    (1, 1, "New Year's Day", "New Year", "New Year obs"),
    (6, 19, "Juneteenth", "Juneteenth", "June 19 obs"),
    (7, 4, "Independence Day", "July 4th", "July 4th obs"),
    (11, 11, "Veterans Day", "Veterans", "Veterans obs"),
    (12, 25, "Christmas Day", "Christmas", "Xmas obs"),
]
JUNETEENTH_FROM = 2021

# (month, weekday, nth (-1 = last), name, short) — always on a weekday.
FLOATING = [
    (1, MON, 3, "Martin Luther King Jr. Day", "MLK Day"),
    (2, MON, 3, "Presidents' Day", "Presidents"),
    (5, MON, -1, "Memorial Day", "Memorial"),
    (9, MON, 1, "Labor Day", "Labor Day"),
    (10, MON, 2, "Columbus Day", "Columbus"),
    (11, THU, 4, "Thanksgiving", "Thanksgiving"),
]


def nth_weekday(year: int, month: int, weekday: int, nth: int) -> dt.date:
    """The `nth` `weekday` of the month; `nth == -1` is the last one."""
    if nth > 0:
        first = dt.date(year, month, 1)
        return first + dt.timedelta(days=(weekday - first.weekday()) % 7 + 7 * (nth - 1))
    nxt = dt.date(year + (month == 12), month % 12 + 1, 1)
    last = nxt - dt.timedelta(days=1)
    return last - dt.timedelta(days=(last.weekday() - weekday) % 7)


def federal_holidays(year: int) -> list[Holiday]:
    """Every federal holiday *in* `year`, observed weekdays included, by date.

    An observed day can cross a year boundary — New Year's Day on a Saturday is
    observed on 31 December of the year before — so this looks at the next
    year's 1 January too, and only returns dates inside `year`.
    """
    out: list[Holiday] = []
    for y in (year, year + 1):
        for month, day, name, short, obs_short in FIXED:
            if name == "Juneteenth" and y < JUNETEENTH_FROM:
                continue
            d = dt.date(y, month, day)
            out.append(Holiday(d, name, short))
            if d.weekday() == SAT:
                out.append(Holiday(d - dt.timedelta(days=1), f"{name} (observed)", obs_short))
            elif d.weekday() == SUN:
                out.append(Holiday(d + dt.timedelta(days=1), f"{name} (observed)", obs_short))
        if y == year:
            for month, weekday, nth, name, short in FLOATING:
                out.append(Holiday(nth_weekday(y, month, weekday, nth), name, short))
    return sorted((h for h in out if h.date.year == year), key=lambda h: h.date)


def holidays_between(start: dt.date, end: dt.date) -> list[Holiday]:
    """Every federal holiday in [start, end], by date."""
    return [
        h
        for y in range(start.year, end.year + 1)
        for h in federal_holidays(y)
        if start <= h.date <= end
    ]
