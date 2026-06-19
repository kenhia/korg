//! korg-migrate — one-shot, fidelity-verified import of kwi + kcard data
//! into korg. Reads frozen `pg_dump` snapshots (sources never mutated),
//! writes into a fresh korg database, and verifies invariants F1-F7.

pub mod import;
pub mod source;
