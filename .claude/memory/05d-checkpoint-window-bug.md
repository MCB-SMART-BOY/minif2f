---
name: checkpoint-data-loss-window
description: FIXED 2026-06-21. mark_done moved out of flush_batch to after write_flat_json so checkpoint never gets ahead of durable JSON data. Was dropping up to 19 theorems on crash.
metadata:
  type: project
---

## Checkpoint / Incremental-Write Ordering Bug — FIXED 2026-06-21

**Severity: MEDIUM — silent data loss on crash, not a panic.**

### Mechanism (pipeline.rs:185-204)
```
flush_batch()              # per theorem
  results.insert(...)      # data → memory
  mark_done(theorem)       # checkpoint → DISK (checkpoint.rs:45, every theorem)
thms_since_write += 1
if thms_since_write >= 20: # INCREMENTAL_WRITE_EVERY
  write_flat_json(...)     # data → DISK (only every 20 theorems)
```

Checkpoint is written **per theorem**; JSON data only **every 20 theorems**.
Crash in between → checkpoint says T1..T19 done, JSON lacks them → on resume
they are skipped by `is_done()` but absent from output → **lost up to 19×128 =
2432 results (3.9% of a model)**.

### Verified live exposure (2026-06-21)
goedel-v2 mid-run: checkpoint=153 theorems, raw JSON=140 → **13 theorems
currently in the crash window**. A shutdown right now would lose those 13.
(This likely bit us during the earlier mid-run machine shutdown.)

### Fix (apply when convenient — needs rerun? no, just future-proofs)
Reorder so data is durable before the theorem is marked done. Options:
1. `write_flat_json` BEFORE `mark_done` (move the write inside flush, every theorem) — simplest, slight I/O cost (JSON is 100s of MB, written whole each time → expensive every theorem).
2. Set INCREMENTAL_WRITE_EVERY=1 — same I/O concern.
3. Best: write JSON every N theorems, but only `mark_done` the theorems that are
   actually in the just-written JSON. I.e. batch the checkpoint to match the
   data write. Decouples the two so checkpoint never gets ahead of durable data.

Not urgent for correctness of completed models (they finished + final write).
Matters for in-progress runs and crash recovery. Related: [[03-pipeline]].

### FIX APPLIED (2026-06-21)
Chose option 3. `flush_batch` no longer checkpoints (params `checkpoint_dir`/
`model_name`/`run_id` removed). The `run` loop buffers flushed theorem names in
`pending_checkpoint`, and `mark_done`s them only AFTER `write_flat_json` succeeds
(both in the every-20 incremental block and the final write). Invariant: the
checkpoint can never name a theorem whose data isn't already on disk. Worst case
on crash: data on disk but theorem unmarked → regenerated on resume (harmless).
Regression test: `test_checkpoint_never_ahead_of_data` (pipeline.rs). 73/73 pass.

Takes effect on the NEXT process start (rerun dpo/deepseek-v2, STP). The live
goedel-v2 still runs the pre-fix binary — do not restart it just for this.
