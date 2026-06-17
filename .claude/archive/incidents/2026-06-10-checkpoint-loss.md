---
name: checkpoint-resume-fix
description: Checkpoint resume loads existing JSON and merges — no data loss on restart
metadata:
  type: project
---

Prior to the fix, checkpoint resume had a data-loss bug: when resuming with the same `--run-id`, theorems already in the checkpoint were skipped (`continue`) but NOT added to the results BTreeMap. The final JSON only contained theorems from the CURRENT run, silently dropping all prior work.

**Fix** (`pipeline.rs:68-72`): On startup, `load_existing_results()` reads the existing output JSON (if present) and populates the results BTreeMap from it. Newly-generated theorems are merged in. Previously-completed theorems are preserved without regeneration.

**How it works:**
1. `load_existing_results(json_path, model_name)` checks if output JSON exists
2. If yes, parses `{model: {theorem: {attempt_N: proof}}}` and populates BTreeMap
3. During generation loop, checkpointed theorems are skipped (already in results)
4. New theorems overwrite whatever was there (fresh run)
5. Final write includes both prior + new results

**Why:** The original logic built results fresh each run. When checkpoint says "already done", the theorem was skipped entirely — no attempt to reload prior proofs from disk.

**How to apply:** Use same `--run-id` on restart. No special flags needed.
