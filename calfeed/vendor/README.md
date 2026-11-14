# Vendored from kdashdata

Two files, copied byte for byte from kdashdata at **b1ad6af** (sprint 025,
the contract slice of program korg:4669), so this repo's gate can validate
korg-calendar's record with nothing installed:

| File | kdashdata path |
|---|---|
| `kdash-calendar.schema.json` | `contracts/schemas/kdash-calendar.schema.json` |
| `jsonschema_mini.py` | `scripts/jsonschema_mini.py` |

kdashdata owns both. Do not edit them here. `test_vendored_files_are_the_pinned_ones`
fails on any byte of drift. To re-pin, copy both from the new commit
(`git -C ~/src/tools/kdashdata show <sha>:<path>`), update the digests in
`PINNED` and the commit above, and run `just calendar-check`. A breaking change
to the contract is a new key (`kdash:calendar:v2`, CD-30), not a re-pin.
