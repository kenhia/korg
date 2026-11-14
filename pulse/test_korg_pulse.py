"""korg-pulse against the kdashdata contract (WI 4611 acceptance, first half).

Run with `just pulse-check` (stdlib only: `python3 -m unittest discover -s pulse`).
"""

from __future__ import annotations

import copy
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
import korg_pulse  # noqa: E402

SCHEMA_PATH = HERE / "vendor" / "kdash-korg-pulse.schema.json"
SCHEMA = json.loads(SCHEMA_PATH.read_text())
TS = 1794617100.4

#: The vendored files, byte for byte as kdashdata d57e1dc has them. A change
#: here is a deliberate re-pin (see vendor/README.md), never an edit in place.
PINNED = {
    "kdash-korg-pulse.schema.json": "43429338633e5586fbaca2424b3d2f02ca360f2bf0977d9987326a1b38031741",
    "jsonschema_mini.py": "d1a649261b9aa8bd6ab871407ab46461cd63be03019aeee8c2f810ee3673eb76",
}


def fixture(name: str) -> dict:
    return json.loads((HERE / "fixtures" / f"{name}.json").read_text())


def flow_of(*backlogs: int) -> dict:
    days = [{"day": f"2026-{10 + i // 28:02d}-{1 + i % 28:02d}", "backlog": b} for i, b in enumerate(backlogs)]
    return {"days": days}


class Contract(unittest.TestCase):
    def assertValid(self, record: dict) -> None:
        self.assertEqual(jsonschema_mini.validate(record, SCHEMA), [])
        # The producer obligation the validator cannot check (no arithmetic, CD-24).
        self.assertEqual(sum(p["open"] for p in record["by_project"]) + record["other_open"], record["open"])
        if record["history"]:
            self.assertEqual(record["history"][-1]["open"], record["open"])

    def test_vendored_files_are_the_pinned_ones(self):
        for name, digest in PINNED.items():
            got = hashlib.sha256((HERE / "vendor" / name).read_bytes()).hexdigest()
            self.assertEqual(got, digest, f"vendor/{name} differs from kdashdata d57e1dc")

    def test_validator_is_live(self):
        # The schema's own examples pass and its counterexamples fail; if the
        # validator accepted everything this suite would prove nothing.
        self.assertEqual(jsonschema_mini.check_schema_file(SCHEMA_PATH.name, SCHEMA), [])

    def test_fixture_board_builds_a_valid_record(self):
        rec = korg_pulse.build_record(fixture("board"), fixture("flow"), TS, 300)
        self.assertValid(rec)
        self.assertEqual(rec["ts"], TS)
        self.assertEqual(rec["expected_refresh_s"], 300)
        self.assertEqual(rec["open"], 50)
        self.assertEqual(rec["awaiting"], 2)
        self.assertEqual(rec["active"], 2)
        self.assertEqual(rec["queued"], 3, "the parked queue row is not startable")

    def test_by_project_is_capped_largest_first_ties_by_name(self):
        rec = korg_pulse.build_record(fixture("board"), fixture("flow"), TS, 300)
        names = [p["name"] for p in rec["by_project"]]
        self.assertEqual(names[:5], ["kligd", "k-homelab", "korg", "agent-skills", "kdashdata"])
        self.assertEqual(len(names), 24)
        self.assertNotIn("empty", names)
        # 27 projects hold items (26 + an archived one still holding one); 3
        # fell past the cap, 3 more items have no project.
        self.assertEqual(rec["projects"], 27)
        self.assertEqual(rec["other_open"], 3 + 3)

    def test_history_is_copied_oldest_first(self):
        rec = korg_pulse.build_record(fixture("board"), fixture("flow"), TS, 300)
        self.assertEqual([h["open"] for h in rec["history"]], [41, 44, 44, 46, 48, 47, 50])
        self.assertEqual(rec["history"][0]["date"], "2026-11-07")

    def test_history_keeps_the_newest_thirty(self):
        board = fixture("board")
        flow = flow_of(*range(10, 49), 50)
        rec = korg_pulse.build_record(board, flow, TS, 300)
        self.assertValid(rec)
        self.assertEqual(len(rec["history"]), 30)
        self.assertEqual(rec["history"][-1]["open"], 50)

    def test_an_empty_korg_is_valid(self):
        board = fixture("board")
        for row in board["depth"]:
            row["wi_total"] = 0
        board.update(active=[], queue=[], awaiting=[])
        rec = korg_pulse.build_record(board, flow_of(0), TS, 60)
        self.assertValid(rec)
        self.assertEqual((rec["open"], rec["projects"], rec["by_project"]), (0, 0, []))

    def test_a_torn_read_is_refused(self):
        # depth counts 47, so a backlog of 40 means a write landed between reads.
        with self.assertRaises(korg_pulse.Inconsistent):
            korg_pulse.build_record(fixture("board"), flow_of(40), TS, 300)

    def test_no_days_is_refused(self):
        with self.assertRaises(korg_pulse.Inconsistent):
            korg_pulse.build_record(fixture("board"), {"days": []}, TS, 300)

    def test_snapshot_rereads_a_torn_pair_once(self):
        good = (fixture("board"), fixture("flow"))
        torn = (fixture("board"), flow_of(40))
        with mock.patch.object(korg_pulse, "read_korg", side_effect=[torn, good]) as rk:
            rec = korg_pulse.snapshot("http://korg", 300)
        self.assertEqual(rk.call_count, 2)
        self.assertValid(rec)
        with mock.patch.object(korg_pulse, "read_korg", side_effect=[torn, copy.deepcopy(torn)]):
            with self.assertRaises(korg_pulse.Inconsistent):
                korg_pulse.snapshot("http://korg", 300)

    def test_main_writes_through_kdash_pub(self):
        good = (fixture("board"), fixture("flow"))
        with mock.patch.object(korg_pulse, "read_korg", return_value=good), \
                mock.patch.object(korg_pulse.subprocess, "run") as run, \
                mock.patch("sys.stdout"):
            self.assertEqual(korg_pulse.main(["--korg", "http://korg"]), 0)
        argv = run.call_args.args[0]
        self.assertEqual(argv[:5], ["kdash-pub", "--app", "korg-pulse", "set", "kdash:korg:pulse"])
        self.assertValid(json.loads(argv[5]))


if __name__ == "__main__":
    unittest.main()
