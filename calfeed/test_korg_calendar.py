"""korg-calendar against the kdashdata contract (WI 4664).

Run with `just calendar-check` (stdlib only: `python3 -m unittest discover -s calfeed`).
Tags here are made up: Ken's own days live only in korg's database.
"""

from __future__ import annotations

import datetime as dt
import hashlib
import json
import pathlib
import sys
import unittest
from unittest import mock

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / "vendor"))

import jsonschema_mini  # noqa: E402
import korg_calendar  # noqa: E402
import us_holidays  # noqa: E402

SCHEMA = json.loads((HERE / "vendor" / "kdash-calendar.schema.json").read_text())

#: The vendored files, byte for byte as kdashdata b1ad6af has them. A change
#: here is a deliberate re-pin (see vendor/README.md), never an edit in place.
PINNED = {
    "kdash-calendar.schema.json": "f986c83074add31f098780ad944ed68f00ed3548054a6569be7b33c31e0df091",
    "jsonschema_mini.py": "d1a649261b9aa8bd6ab871407ab46461cd63be03019aeee8c2f810ee3673eb76",
}


def ts_at(iso: str) -> float:
    return dt.datetime.fromisoformat(iso).timestamp()


#: 2026-11-14 09:00 PST, a Saturday.
NOV14 = ts_at("2026-11-14T09:00:00-08:00")

TAGS = [
    {"id": 1, "date": "2026-11-25", "kind": "off", "yearly": False, "label": None, "short": None},
    {"id": 2, "date": "2026-11-27", "kind": "off", "yearly": False, "label": None, "short": None},
    {"id": 3, "date": "1990-11-27", "kind": "special", "yearly": True,
     "label": "Sam's birthday", "short": "Sam"},
]


def valid(record: dict) -> list[str]:
    return jsonschema_mini.validate(record, SCHEMA)


def by_date(record: dict) -> dict[str, dict]:
    return {d["date"]: d for d in record["days"]}


class Vendored(unittest.TestCase):
    def test_vendored_files_are_the_pinned_ones(self):
        for name, digest in PINNED.items():
            got = hashlib.sha256((HERE / "vendor" / name).read_bytes()).hexdigest()
            self.assertEqual(got, digest, f"vendor/{name} drifted from kdashdata b1ad6af")

    def test_the_validator_bites(self):
        # Its own examples pass and its counterexamples fail, so a green suite
        # cannot rest on a validator that accepts everything.
        self.assertEqual(jsonschema_mini.check_schema_file("kdash-calendar.schema.json", SCHEMA), [])
        for example in SCHEMA["examples"]:
            self.assertEqual(valid(example), [])
        bad = json.loads(json.dumps(SCHEMA["examples"][0]))
        del bad["days"][0]["off"]
        self.assertNotEqual(valid(bad), [])


class Holidays(unittest.TestCase):
    def test_2026(self):
        got = [(h.date.isoformat(), h.name) for h in us_holidays.federal_holidays(2026)]
        self.assertEqual(got, [
            ("2026-01-01", "New Year's Day"),
            ("2026-01-19", "Martin Luther King Jr. Day"),
            ("2026-02-16", "Presidents' Day"),
            ("2026-05-25", "Memorial Day"),
            ("2026-06-19", "Juneteenth"),
            ("2026-07-03", "Independence Day (observed)"),
            ("2026-07-04", "Independence Day"),
            ("2026-09-07", "Labor Day"),
            ("2026-10-12", "Columbus Day"),
            ("2026-11-11", "Veterans Day"),
            ("2026-11-26", "Thanksgiving"),
            ("2026-12-25", "Christmas Day"),
        ])

    def test_observed_days_cross_weekends_and_years(self):
        def names(year):
            return {h.date.isoformat(): h.name for h in us_holidays.federal_holidays(year)}
        # Saturday New Year's Day 2022 is observed on Friday 31 December 2021.
        self.assertEqual(names(2021)["2021-12-31"], "New Year's Day (observed)")
        self.assertNotIn("2021-12-31", names(2022))
        # Sunday -> Monday.
        self.assertEqual(names(2023)["2023-01-02"], "New Year's Day (observed)")
        self.assertEqual(names(2027)["2027-06-18"], "Juneteenth (observed)")
        self.assertEqual(names(2027)["2027-12-24"], "Christmas Day (observed)")
        self.assertEqual(names(2028)["2028-11-10"], "Veterans Day (observed)")
        # Juneteenth starts in 2021.
        self.assertNotIn("2020-06-19", names(2020))

    def test_last_and_nth_weekdays(self):
        self.assertEqual(us_holidays.nth_weekday(2027, 5, us_holidays.MON, -1), dt.date(2027, 5, 31))
        self.assertEqual(us_holidays.nth_weekday(2025, 11, us_holidays.THU, 4), dt.date(2025, 11, 27))
        self.assertEqual(us_holidays.nth_weekday(2026, 12, us_holidays.MON, -1), dt.date(2026, 12, 28))

    def test_every_name_and_short_fits_the_contract(self):
        for year in range(2021, 2061):
            for h in us_holidays.federal_holidays(year):
                self.assertLessEqual(len(h.name), 31, h)
                self.assertLessEqual(len(h.short), 12, h)
                self.assertLess(h.date.weekday(), 7)


class Record(unittest.TestCase):
    def test_this_november(self):
        rec = korg_calendar.build_record(TAGS, NOV14)
        self.assertEqual(valid(rec), [])
        self.assertEqual((rec["from"], rec["to"]), ("2026-11-01", "2027-02-14"))
        self.assertEqual(rec["expected_refresh_s"], 3600)
        self.assertNotIn("v", rec)
        days = by_date(rec)
        self.assertEqual(days["2026-11-11"],
                         {"date": "2026-11-11", "off": False, "holiday": "Veterans Day", "short": "Veterans"})
        self.assertEqual(days["2026-11-25"], {"date": "2026-11-25", "off": True})
        self.assertEqual(days["2026-11-26"],
                         {"date": "2026-11-26", "off": False, "holiday": "Thanksgiving", "short": "Thanksgiving"})
        self.assertEqual(days["2026-11-27"],
                         {"date": "2026-11-27", "off": True, "special": "Sam's birthday", "short": "Sam"})
        # A holiday nobody tagged is not a day off.
        self.assertEqual(days["2026-12-25"]["off"], False)
        self.assertEqual(days["2027-01-18"]["short"], "MLK Day")
        dates = [d["date"] for d in rec["days"]]
        self.assertEqual(dates, sorted(set(dates)), "sorted, each date once")
        self.assertTrue(all(rec["from"] <= d <= rec["to"] for d in dates))

    def test_dates_are_los_angeles_not_utc(self):
        # 21:00 PST on 30 November is already 1 December in UTC.
        rec = korg_calendar.build_record([], ts_at("2026-11-30T21:00:00-08:00"))
        self.assertEqual(rec["from"], "2026-11-01")
        rec = korg_calendar.build_record([], ts_at("2026-12-01T00:30:00-08:00"))
        self.assertEqual(rec["from"], "2026-12-01")

    def test_the_window_is_ninety_days_even_dated_in_utc(self):
        ts = ts_at("2026-11-14T20:00:00-08:00")
        rec = korg_calendar.build_record([], ts)
        utc_day = dt.datetime.fromtimestamp(ts, dt.timezone.utc).date()
        self.assertGreaterEqual(dt.date.fromisoformat(rec["to"]) - utc_day, dt.timedelta(days=90))

    def test_yearly_repeats_from_its_own_year_and_handles_leap_day(self):
        leap = {"id": 9, "date": "2024-02-29", "kind": "special", "yearly": True,
                "label": "Leap", "short": None}
        later = {"id": 8, "date": "2027-01-05", "kind": "special", "yearly": True,
                 "label": "Not yet", "short": None}
        rec = korg_calendar.build_record([leap, later], ts_at("2027-01-10T12:00:00-08:00"))
        days = by_date(rec)
        self.assertEqual(days["2027-02-28"]["special"], "Leap")
        self.assertIn("2027-01-05", days)
        rec = korg_calendar.build_record([later], ts_at("2026-11-14T12:00:00-08:00"))
        self.assertNotIn("2026-01-05", by_date(rec))

    def test_a_special_short_wins_over_a_holiday_and_the_holiday_is_the_fallback(self):
        tags = [
            {"id": 1, "date": "2026-12-25", "kind": "special", "yearly": False,
             "label": "Anniversary", "short": "Anniv"},
            {"id": 2, "date": "2026-11-26", "kind": "special", "yearly": False,
             "label": "Family visit", "short": None},
        ]
        days = by_date(korg_calendar.build_record(tags, NOV14))
        self.assertEqual(days["2026-12-25"]["short"], "Anniv")
        self.assertEqual(days["2026-12-25"]["holiday"], "Christmas Day")
        self.assertEqual(days["2026-11-26"]["short"], "Thanksgiving")

    def test_two_specials_join_and_fit(self):
        tags = [
            {"id": i, "date": "2026-12-02", "kind": "special", "yearly": False,
             "label": label, "short": None}
            for i, label in enumerate(["A long first name of a day", "Another one"])
        ]
        rec = korg_calendar.build_record(tags, NOV14)
        self.assertEqual(valid(rec), [])
        special = by_date(rec)["2026-12-02"]["special"]
        self.assertEqual(len(special), 31)
        self.assertTrue(special.startswith("A long first name of a day / "))
        self.assertNotIn("short", by_date(rec)["2026-12-02"])

    def test_at_most_64_days_keeping_the_earliest(self):
        tags = [{"id": i, "date": (dt.date(2026, 11, 1) + dt.timedelta(days=i)).isoformat(),
                 "kind": "off", "yearly": False, "label": None, "short": None} for i in range(100)]
        rec = korg_calendar.build_record(tags, NOV14)
        self.assertEqual(len(rec["days"]), 64)
        self.assertEqual(rec["days"][0]["date"], "2026-11-01")
        self.assertEqual(valid(rec), [])

    def test_nothing_tagged_still_carries_the_holidays(self):
        rec = korg_calendar.build_record([], NOV14)
        self.assertTrue(rec["days"])
        self.assertTrue(all(not d["off"] for d in rec["days"]))


class Publishing(unittest.TestCase):
    def setUp(self):
        self.rec = korg_calendar.build_record(TAGS, NOV14)

    def test_absent_changed_fresh_and_stale(self):
        live = dict(self.rec)
        self.assertEqual(korg_calendar.should_publish(self.rec, None, NOV14), "absent")
        self.assertIsNone(korg_calendar.should_publish(self.rec, live, NOV14 + 60))
        self.assertEqual(korg_calendar.should_publish(self.rec, live, NOV14 + 3600), "refresh")
        changed = dict(live, days=live["days"][1:])
        self.assertEqual(korg_calendar.should_publish(self.rec, changed, NOV14 + 60), "changed")
        rolled = dict(live, to="2027-02-13")
        self.assertEqual(korg_calendar.should_publish(self.rec, rolled, NOV14 + 60), "changed")

    def run_main(self, live_rc, live_out, *argv):
        calls = []

        def fake_run(cmd, **kw):
            calls.append(cmd)
            if cmd[-2:] == ["get", korg_calendar.KEY]:
                return mock.Mock(returncode=live_rc, stdout=live_out)
            return mock.Mock(returncode=0)

        with mock.patch.object(korg_calendar, "read_tags", return_value=TAGS), \
             mock.patch.object(korg_calendar.time, "time", return_value=NOV14), \
             mock.patch.object(korg_calendar.subprocess, "run", side_effect=fake_run), \
             mock.patch("sys.stdout"):
            self.assertEqual(korg_calendar.main(list(argv)), 0)
        return calls

    def test_main_writes_with_kdash_pub_when_unset(self):
        calls = self.run_main(1, "", "--kdash-pub", "/x/kdash-pub")
        cmd = calls[-1]
        self.assertEqual(cmd[:5], ["/x/kdash-pub", "--app", "korg-calendar", "set", "kdash:calendar"])
        self.assertEqual(json.loads(cmd[5]), self.rec)

    def test_main_writes_nothing_when_unchanged(self):
        calls = self.run_main(0, json.dumps(self.rec))
        self.assertEqual([c[3] for c in calls], ["get"])

    def test_force_and_dry_run(self):
        calls = self.run_main(0, json.dumps(self.rec), "--force")
        self.assertEqual(calls[-1][3], "set")
        self.assertEqual(self.run_main(0, "", "--dry-run"), [])


if __name__ == "__main__":
    unittest.main()
