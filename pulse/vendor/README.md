# Vendored from kdashdata

Two files, copied byte for byte from kdashdata at **d57e1dc** (sprint 023,
the contracts slice of program korg:4619), so this repo's gate can validate
korg-pulse's record with nothing installed:

| File | kdashdata path |
|---|---|
| `kdash-korg-pulse.schema.json` | `contracts/schemas/kdash-korg-pulse.schema.json` |
| `jsonschema_mini.py` | `scripts/jsonschema_mini.py` |

kdashdata owns both. Do not edit them here. `test_vendored_files_are_the_pinned_ones`
fails on any byte of drift. To re-pin, copy both from the new commit
(`git -C ~/src/tools/kdashdata show <sha>:<path>`), update the digests in
`PINNED` and the commit above, and run `just pulse-check`. A breaking change
to the contract is a new key (`kdash:korg:v2:pulse`, CD-28), not a re-pin.
