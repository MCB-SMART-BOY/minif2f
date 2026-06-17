# minif2f Architecture — Full Logical Chain

## Overview

**Goal**: Generate 128 proof attempts per theorem for miniF2F (488 theorems) using 6 Lean 4 theorem-proving LLMs using vLLM with FP8 quantization on a single RTX 5090 32GB GPU.

**Stack**: Rust orchestrator + vLLM (Python, managed via `uv` venv) for 5 models + HF `model.generate()` for STP. FP8 quantization.

**Output**: Two flat JSON files per model:
- `output/raw_output/<model>.json` — unfiltered model completions
- `output/lean_code/<model>.json` — extracted + assembled Lean proofs (empty string if invalid)

---

## File Map (9 files, ~650 LOC)

| File | LOC | Role |
|------|-----|------|
| `main.rs` | 128 | CLI entry point (clap derive) |
| `lib.rs` | 7 | Module declarations |
| `config.rs` | 72 | `ModelConfig` + `PipelineConfig` structs |
| `models.rs` | 222 | 6-model registry with per-model specs |
| `data.rs` | 158 | `Theorem` struct, JSONL loader, `make_proof_file()` |
| `prompts.rs` | 910 | Chat templates + 5 prompt formats + proof extraction + validation |
| `inference.rs` | 289 | `InferenceEngine`: vLLM server lifecycle, HTTP `/v1/completions` |
| `checkpoint.rs` | 140 | Atomic JSON-set crash recovery |
| `pipeline.rs` | 428 | Continuous request pool → two-layer JSON output |

---

## Layer 1: Configuration (`config.rs`)

### `ModelConfig` (serde Deserialize/Serialize)
Per-model inference parameters, aligned to official HuggingFace specs.

| Field | Type | Purpose |
|-------|------|---------|
| `name` | String | CLI name, e.g. `"kimina-prover-rl-1.7b"` |
| `hf_repo` | String | HuggingFace repo, e.g. `"AI-MO/Kimina-Prover-RL-1.7B"` |
| `architecture` | String | Prompt wrapper: `"qwen3"` \| `"deepseek_v2"` \| `"deepseek_coder"` \| `"raw"` |
| `prompt_format` | String | User message format: `"kimina"` \| `"goedel_v2"` \| `"simple"` \| `"deepseek_prover"` |
| `param_count_b` | Option\<f64\> | Billion parameters (for display) |
| `quantization` | Option\<String\> | e.g. `"awq"`, `"q4_k_m"` |
| `max_model_len` | u32 | Max context length for vLLM `--max-model-len` |
| `max_tokens` | u32 | Max output tokens per completion |
| `temperature` | f64 | Sampling temperature (default 0.6) |
| `top_p` | f64 | Nucleus sampling (default 0.95) |
| `seed` | u64 | Base seed (per-attempt seed = base + attempt_index) |
| `stop_sequences` | Vec\<String\> | Stop tokens sent to vLLM `/v1/completions` `stop` parameter |
| `system_prompt` | String | System message prepended to chat template (empty for raw/no-system models) |

### `PipelineConfig` (Default)
Runtime configuration for a single pipeline run.

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `project_root` | PathBuf | `"."` | Root directory for data/output/checkpoint resolution |
| `uv_project_dir` | String | `"tools/vllm"` | Path to vLLM Python project |
| `port` | u16 | `8080` | HTTP port for vLLM server |
| `completion_attempts` | usize | `128` | Number of attempts per theorem |
| `parallel` | u32 | `8` | vLLM `--max-num-seqs` (continuous batching) |

### Helper methods on `PipelineConfig`:
- **`data_path()`** → `project_root/data` — where JSONL theorem files live
- **`output_dir()`** → `project_root/output` — where raw_output/ and lean_code/ go
- **`checkpoint_dir()`** → `project_root/results/checkpoints` — crash recovery state
- **`uv_project_dir()`** → path to `tools/vllm/` for vLLM Python server

---

## Layer 2: Model Registry (`models.rs`)

### `defaults()` → `ModelConfig`
Returns a template `ModelConfig` with common defaults (qwen3 arch, kimina format, temp=0.6, top_p=0.95, seed=42, stop=`<|im_end|>`+`</s>`). Each model overrides via `..defaults()`.

### `builtin_models()` → `Vec<ModelConfig>`
Returns all 6 models with official specs. Each entry is a `ModelConfig` struct literal with `..defaults()` spread.

#### Model-by-model spec table:

| CLI Name | Arch | Base Model | ctx | max_tok | temp | top_p | seed | Prompt Format | Sys Prompt |
|----------|------|-----------|-----|---------|------|-------|------|---------------|------------|
| `goedel-prover-dpo` | raw | LLaMA-7B | 4096 | 2048 | 1.0 | 0.95 | 1 | simple | _(empty)_ |
| `kimina-prover-rl-1.7b` | qwen3 | Qwen3-1.7B | 40960 | 8096 | 0.6 | 0.95 | 42 | kimina | expert math+Lean4 |
| `goedel-prover-v2-8b` | qwen3 | Qwen3-8B | 40960 | 32768 | 0.6 | 0.95 | 30 | goedel_v2 | _(empty)_ |
| `deepseek-prover-v2-7b` | deepseek_v2 | LLaMA-7B | 65536 | 8192 | 0.6 | 0.95 | 30 | goedel_v2_nocot | _(empty)_ |
| `kimina-prover-distill-8b` | qwen3 | Qwen3-8B | 40960 | 8096 | 0.6 | 0.95 | 42 | kimina | expert math+Lean4 |
| `stp-model-lean` | raw | DS-Prover-V1.5 | 1024 | 1024 | 1.0 | 1.0 | 1 | deepseek_prover | _(empty)_ |

### `find_model(name: &str)` → `Option<ModelConfig>`
Looks up a model config by CLI name. Used by `main.rs` to resolve the `--model` argument.

### `list_model_names()` → `Vec<String>`
Returns all 6 model names. Used by `list-models` and `status` CLI commands.

---

## Layer 3: Data Layer (`data.rs`)

### `Theorem` (serde Deserialize)
A single theorem from the JSONL dataset. Fields:
- `name`: e.g. `"mathd_numbertheory_185"` — unique identifier
- `split`: `"test"` or `"valid"` — dataset split
- `header`: Lean imports + options (e.g. `"import Mathlib\nopen Nat"`)
- `informal_prefix`: `/-- Natural language problem description -/`
- `formal_statement`: `"theorem name (args) : conclusion := by"` — the formal spec
- `goal`: the goal proposition (rarely used)

### `Theorem::make_proof_file(&self, proof_body: &str)` → `String`
Assembles a complete Lean source file from theorem parts:
```
{header}
{informal_prefix}
{formal_statement}
{proof_body}
```
Returns the concatenated string. Parts are only included if non-empty.

### `load_jsonl(path: &Path, filter_split: Option<&str>)` → `Result<Vec<Theorem>>`
Reads a JSONL file line-by-line, deserializing each line as a `Theorem`. If `filter_split` is provided, only theorems matching that split are included.

### `load_split(data_dir: &Path, split: &str)` → `Result<Vec<Theorem>>`
Loads theorems for a specific split (`"test"` or `"valid"`). Tries:
1. `data/raw/{split}.jsonl` — per-split file
2. `data/raw/minif2f.jsonl` — combined file, filtered by split

### `load_all(data_dir: &Path)` → `Result<Vec<Theorem>>`
Loads ALL theorems from both `"test"` and `"valid"` splits. Merges them into one `Vec<Theorem>`. Errors only if both splits fail to load.

---

## Layer 4: Prompt Building + Proof Extraction (`prompts.rs`)

This is the largest and most complex file (~910 lines). It has two responsibilities:
1. **Build prompts** — format user messages with chat templates
2. **Extract and validate proofs** — multi-strategy extraction from raw model output

### `PromptBuilder` struct
```rust
struct PromptBuilder { config: ModelConfig }
```

### `PromptBuilder::new(config: ModelConfig)` → `Self`
Constructor — stores the model config.

---

### Prompt Building Chain

#### `PromptBuilder::build(&self, theorem: &Theorem)` → `String`
**Top-level entry point.** Constructs the full prompt sent to vLLM:

1. Calls `build_user_message(theorem)` → gets the user content
2. Wraps it in the model's chat template based on `config.architecture`

**Chat templates** (4 architectures):

| Architecture | Template | Used by |
|-------------|----------|---------|
| `qwen3` | `<\|im_start\|>system\n{sys}<\|im_end\|>\n<\|im_start\|>user\n{user}<\|im_end\|>\n<\|im_start\|>assistant\n` | Kimina-RL, Goedel-V2, Kimina-Distill |
| `deepseek_v2` | `{sys}<｜User｜>{user}<｜Assistant｜>` | DeepSeek-Prover-V2 |
| `deepseek_coder` | `{sys}### Instruction:\n{user}\n### Response:\n` | Legacy DeepSeek Coder support |
| `raw` | `{user}` (bare, no template) | Goedel-Prover-DPO, STP |

**Special cases in templates:**
- **Qwen3**: Do not prepopulate an empty `<think>` block. Kimina models generate their own reasoning block; Goedel-V2 uses the official proof-plan prompt instead. When `system_prompt.is_empty()` (Goedel-V2), the system message block is entirely omitted — matching official `apply_chat_template` behavior.
- **DeepSeek V2**: BOS (`<｜begin▁of▁sentence｜>`) is NOT included in the template — vLLM adds it automatically via `add_bos_token` in the tokenizer config. Including it would produce a double BOS warning.
- **Goedel-DPO raw format**: Uses the official Goedel-Prover eval prompt directly, with an open ```lean4 block and no chat wrapper.

#### `PromptBuilder::build_user_message(&self, theorem: &Theorem)` → `String`
Routes to the format-specific builder based on `config.prompt_format`:

| Format | Function | Used by |
|--------|----------|---------|
| `kimina` | `build_kimina()` | Kimina-RL, Kimina-Distill |
| `goedel_v2` | `build_goedel_v2(theorem, true)` | Goedel-V2 (CoT) |
| `goedel_v2_nocot` | `build_goedel_v2(theorem, false)` | DeepSeek-Prover-V2 (non-CoT) |
| `simple` | `build_simple()` | Goedel-Prover-DPO |
| `deepseek_prover` | `build_deepseek_prover()` | STP |

#### `build_kimina(theorem)` → `String`
~~~text
Think about and solve the following problem step by step in Lean 4.
# Problem:{description from informal_prefix}
# Formal statement:
```lean4
{header}
{informal_prefix}
{formal_statement}
```
~~~
- NO `sorry` — theorem ends with `:= by`
- Model expected to output `<think>...</think>` followed by a ```lean4 block
- Do NOT prepopulate an empty `<think>` block — the model generates it naturally

#### `build_goedel_v2(theorem, cot)` → `String`
When `cot=true` (Goedel-Prover-V2):
~~~text
Complete the following Lean 4 code:

```lean4
{header}
{informal_prefix}
{formal_statement}
  sorry
```
Before producing the Lean 4 code to formally prove the given theorem, provide a detailed proof plan...
~~~
When `cot=false` (non-CoT, used by DeepSeek-Prover-V2):
~~~text
Complete the following Lean 4 code:

```lean4
{header}
{informal_prefix}
{formal_statement}
  sorry
```
~~~
- Includes `sorry` placeholder — model must replace it
- Official format: closing ``` on its own line after `sorry`
- CoT mode asks for proof plan; non-CoT outputs code directly

#### `build_simple(theorem)` → `String`
~~~text
Complete the following Lean 4 code with explanatory comments preceding each line of code:

```lean4
{header}
{informal_prefix}
{formal_statement}
~~~
- Raw completion prompt with an open code block
- Model generates Lean tactics from `:= by`

#### `build_deepseek_prover(theorem)` → `String`
~~~text
Complete the following Lean 4 code:

```lean4
{header}
{formal_statement}
~~~
- **Excludes** `informal_prefix` — STP has only 1024 context tokens
- Strips trailing `sorry` from `formal_statement` (official: `rsplit("sorry", 1)[0].strip()`)
- Raw completion prompt with an open code block; model generates Lean tactics from `:= by`

---

### Proof Extraction Chain

#### `PromptBuilder::extract_proof(&self, raw: &str)` → `String`
**Top-level proof extraction.** Returns only the proof body (tactics after `:= by`), or empty string if nothing valid found.

**Strategy (prioritized):**

1. **Find ```lean4 block after `</think>`** — primary format for Kimina models
   - Find `</think>` position → search from there for a fenced code block
   - Strip theorem header → validate `has_proof_body()` → return if valid

2. **Fallback: any ```lean4 block in entire text**
   - Search entire text for any fenced code block
   - Same strip+validate pipeline

3. **Fallback: extract Lean tactics from raw text**
   - Look for `:= by` in raw text
   - Collect indented/non-indented tactic lines after it
   - Reconstruct context (theorem statement leading to `:= by`)
   - Validate with `has_proof_body()`

4. **Last resort: strip everything, validate remains**
   - Strip `<think>...</think>` blocks
   - Strip all known chat tokens (`<|im_end|>`, `<｜User｜>`, `### Instruction:`, etc.)
   - Strip markdown headers (`# `, `## `, `**`)
   - Strip trailing ``` fence
   - Validate `has_proof_body()` → return if valid, else `""`

#### `PromptBuilder::validate_lean_code(&self, code: &str)` → `bool`
**Post-extraction validation.** Returns `true` only if the assembled Lean code is a complete, compilable proof file.

Checks (all must pass):
1. Code is non-empty
2. Contains `:= by` (theorem has a proof block)
3. Does NOT contain `sorry` (proof is complete)
4. Has ≥2 characters of tactics after `:= by`
5. No markdown artefacts in proof body (` ``` `, `**`)
6. No chat tokens in proof body (`<|im_start|>`, `<｜User｜>`, etc.)
7. `is_proof_body()` passes — not natural language commentary
8. After `strip_block_comments()`, ≥2 characters of real tactics remain

---

### Helper Functions

#### `extract_fenced_code(text: &str)` → `Option<String>`
Finds the best fenced code block. Tries multiple fence openers (` ```lean4\n`, ` ```lean4`, ` ```tactics\n`, ` ```\n`, ` ``` `). Returns the block with the most content after stripping the theorem header.

#### `strip_theorem_header(code: &str)` → `String`
Strips everything before `:= by` from the code. Uses **`find` (first occurrence)**, NOT `rfind` — this preserves nested `have ... := by` blocks inside the proof body.

#### `is_proof_body(text: &str)` → `bool`
Detects whether text looks like Lean tactics (vs. natural language commentary). Rejects:
- Theorem/lemma/import header words
- Backtick/markdown fences
- Uppercase-first text with >4 words (English prose), EXCEPT Lean comments (`--`, `/-`)

#### `has_proof_body(code: &str)` → `bool`
Combines `strip_theorem_header()` + minimum length check + `is_proof_body()`. The primary gatekeeper for "is this a real proof?"

#### `strip_block_comments(text: &str)` → `String`
Removes Lean block comments (`/- ... -/`) from text. Handles nested comments with a depth counter. Used by `validate_lean_code` to reject commentary-only "proofs" — if nothing remains after stripping comments, the model generated only natural language.

#### `strip_trailing_fence(text: &str)` → `String`
Strips trailing ``` from proof body. Used for open code block formats (Goedel-DPO, STP) where the model may close the code block.

#### `strip_chat_tokens(text: &str)` → `String`
Removes all known chat template tokens from text (Qwen, DeepSeek V2, DeepSeek Coder, and ASCII variants).

#### `strip_think_blocks(text: &str)` → `String`
Removes `<think>...</think>` blocks. Handles incomplete blocks (model ran out of tokens mid-think) by removing just the opening tag.

#### `strip_markdown_from_proof(code: &str)` → `String`
Filters out lines starting with `# `, `## `, or `**` (markdown commentary).

#### `extract_lean_from_text(text: &str)` → `Option<String>`
Extracts Lean tactics from raw text when no fenced code block is found. Strategy:
- Find `:= by` → collect indented lines after it
- Stop at blank line or new theorem/lemma/import boundary
- Accept indented lines (` ` or `\t`), bullet tactics (`·`, `.`), comments (`--`)
- Accept first non-indented line if short (e.g. `rfl`, `simp_all`)
- Reconstruct theorem context from lines before `:= by`

#### `extract_problem_desc(prefix: &str)` → `String`
Extracts natural language description from `/-- ... -/` comment blocks. Strips the `/--` and `-/` delimiters.

---

## Layer 5: Inference Engine (`inference.rs`)

### `InferenceEngine` struct
```rust
struct InferenceEngine {
    config: ModelConfig,   // model config (for params)
    client: Client,        // reqwest HTTP client (5-min timeout)
    server: Child,         // vLLM child process handle
    base_url: String,      // "http://localhost:{port}"
}
```

### `InferenceEngine::start(config, model_path, port, uv_project_dir, parallel)` → `Result<Self>`
**Spawns vLLM via `uv run` as a child process** and waits for it to be ready.

**vLLM command:**
```
uv run --directory <uv_project_dir> python -m vllm.entrypoints.openai.api_server \
  --model <model_path> --port <port> \
  --max-model-len <per_seq> --max-num-seqs <parallel> \
  --gpu-memory-utilization 0.92 --dtype half --trust-remote-code \
  --quantization fp8 --tokenizer-mode slow \
  --disable-custom-all-reduce --disable-log-stats
```

Environment variables: `CUDA_HOME`, `VLLM_USE_FLASHINFER_SAMPLER=0`, `VLLM_ATTENTION_BACKEND=FLASH_ATTN`, `OMP_NUM_THREADS=""`.

Stderr is redirected to `/tmp/vllm-server-{port}.log`.

**Health check loop**: Polls `GET /health` every 2s. Status 200 = ready. Timeout after 5 minutes → kill server, bail.

### `InferenceEngine::generate_one_with_retry(client, url, body, max_retries)` → `String`
**Static method** — sends a single `POST /v1/completions` request with exponential backoff retry.

- Sends OpenAI-compatible JSON body: `{"prompt", "max_tokens", "temperature", "top_p", "seed", "stop"}`
- Extracts `json["choices"][0]["text"]` as string
- Runs through `decode_llama_byte_fallback()` to fix LLaMA tokenizer byte-fallback encoding
- Retries on HTTP errors and JSON parse errors (up to `max_retries` times)
- Backoff: 1s, 2s, 4s...
- Returns `""` on final failure (graceful degradation)

### `InferenceEngine::generate_stream(&self, prompt, n, attempt_offset)` → `FuturesUnordered`
Returns a **stream** of `(attempt_index, text)` futures. Each future completes independently — no barrier. Each attempt gets a unique seed: `base_seed + attempt_offset + i`.

### `InferenceEngine::generate_batch_retry(&self, prompt, n, attempt_offset)` → `Vec<String>`
**Legacy batch API** — waits for ALL n attempts to complete before returning. Deprecated in favor of the streaming pipeline, but kept for potential use.

### `InferenceEngine::http_client()` → `&Client`
Accessor for the HTTP client — used by `pipeline.rs` to create streaming requests.

### `InferenceEngine::base_url()` → `&str`
Accessor for the base URL — used by `pipeline.rs` to build `/completion` URLs.

### `InferenceEngine::stop(self)`
Kills the vLLM process (SIGTERM → SIGKILL). Frees GPU memory.

### `Drop` implementation
Kills the server if `stop()` wasn't called explicitly. Zombie reaping.

---

## Layer 6: Checkpointing (`checkpoint.rs`)

### `CheckpointManager` struct
```rust
struct CheckpointManager {
    file: PathBuf,                // e.g. "results/checkpoints/kimina-prover-rl-1.7b__v128-20260606.json"
    completed: HashSet<String>,   // theorem names already done
    initial_skipped: usize,       // count at load time (for display)
}
```

### `CheckpointManager::new(checkpoint_dir, model_name, run_id)` → `Result<Self>`
Loads existing checkpoint from `{checkpoint_dir}/{model_name}__{run_id}.json`. If the file exists, deserializes the HashSet of completed theorem names. Otherwise starts with an empty set.

### `CheckpointManager::is_done(&self, name: &str)` → `bool`
Checks whether a theorem's configured attempt batch is already complete.

### `CheckpointManager::mark_done(&mut self, name: &str)` → `Result<()>`
Adds a theorem name to the completed set and **atomically writes** the checkpoint:
1. Serialize to temp file (`{file}.tmp`)
2. Rename temp → real file (atomic on Unix)

### `CheckpointManager::total_done()` → `usize`
Returns the number of completed theorems.

---

## Layer 7: Pipeline Orchestration (`pipeline.rs`)

### Type Aliases
```rust
type AttemptMap = BTreeMap<String, (String, String)>;  // "attempt_1" → (raw_output, lean_code)
type ResultsMap = BTreeMap<String, AttemptMap>;         // theorem_name → AttemptMap
```

### `EvaluationPipeline` struct
```rust
struct EvaluationPipeline {
    config: PipelineConfig,  // runtime settings
    run_id: String,          // checkpoint run ID
}
```

### `EvaluationPipeline::new(config, run_id)` → `Self`
Constructor.

### `EvaluationPipeline::run(&self, model_cfg, model_path)` → `Result<()>`
**Main orchestration function.** The complete pipeline for a single model:

#### Phase 0: Load & Setup
1. **Load theorems**: `load_all()` → 488 theorems
2. **Start inference engine**: `InferenceEngine::start()` → spawns vLLM, waits `/health`
3. **Create output dirs**: `output/raw_output/`, `output/lean_code/`
4. **Init checkpoint**: `CheckpointManager::new()` → resume from prior run
5. **Load existing results**: `load_existing_results()` → merge raw_output + lean_code JSONs from disk
6. **Build pending list**: Skip theorems already marked done in checkpoint

#### Phase 1: Build Job List
For each pending theorem × `completion_attempts`:
- Build prompt via `PromptBuilder::build(theorem)`
- Create JSON body: `{"prompt", "max_tokens", "temperature", "top_p", "seed", "stop"}`
- Seed = `base_seed + attempt_index` (ensures deterministic but diverse sampling)
- Total jobs = `pending_theorems × completion_attempts`

#### Phase 2: Continuous Request Pool (the key architectural decision)
```
buffer_unordered(concurrency) — keeps N HTTP requests in flight at all times.
When one completes, the next job starts immediately.
```

Jobs flow through `buffer_unordered(concurrency)`:
- Each job calls `InferenceEngine::generate_one_with_retry()` → (theorem, attempt_index, model_output_text)
- Results arrive in completion order (NOT submission order)
- GPU utilization stays at ~90%+ — no per-theorem idle gaps

#### Phase 3: Per-Theorem Batch Accumulation
Since `buffer_unordered` interleaves results from different theorems, a `BTreeMap<String, (Theorem, Vec<(usize, String)>)>` accumulates results per theorem. When a theorem's batch reaches `completion_attempts`, it's flushed.

#### Phase 4: Flush → Extraction → Validation → Write
When a theorem's batch is complete:

1. **Parallel extraction** (rayon): `batch.par_iter().map(|(attempt, text)| { ... })`
   - `PromptBuilder::extract_proof(text)` → proof body
   - If proof body is empty → lean = `""`
   - If proof body contains `import ` → use as-is (model generated complete file)
   - Otherwise → `Theorem::make_proof_file(proof_body)` → assemble full Lean code
   - **Validate**: `PromptBuilder::validate_lean_code(lean)` — reject empty/incomplete/wrong proofs
   - Returns `(attempt_index, raw_text, lean_code)`

2. **Sequential insertion**: Results go into `BTreeMap` (not `Sync`, so done sequentially after parallel extraction)

3. **Progress bar**: `bar.inc(1)` per attempt

4. **Checkpoint**: `CheckpointManager::mark_done(theorem_name)` (spawned as blocking task to avoid blocking async runtime)

5. **Incremental JSON write**: Every 20 theorems, write both JSON files to disk (crash resilience)

#### Phase 5: Final Write + Shutdown
- Write final JSON files (catches any theorems after the last incremental write)
- `engine.stop()` → kill vLLM, free GPU
- Print summary: theorem count, file sizes

---

### Key Functions in `pipeline.rs`

#### `flush_batch(results, theorem, batch, pb, checkpoint_dir, model_name, run_id, bar)`
**Parallel batch extraction via rayon, then sequential BTreeMap insert.**
1. `rayon::par_iter()` — splits one theorem's attempts across CPU cores
2. For each attempt: extract proof → assemble Lean → validate → (attempt_index, raw, lean)
3. Sequential insertion into `ResultsMap`
4. Spawns `tokio::task::spawn_blocking` for checkpoint write

#### `write_flat_json(path, model_cfg, results, pick)` → `Result<()>`
Writes a flat JSON file with the structure:
```json
{
  "<model_name>": {
    "<theorem_name>": {
      "attempt_1": "<text>",
      ...
      "attempt_128": "<text>"
    }
  }
}
```
The `pick` function selects either `(raw, _lean) → raw` or `(_raw, lean) → lean` for the two output files.

#### `load_existing_results(raw_path, lean_path, model_name)` → `Result<ResultsMap>`
Reads existing raw_output and lean_code JSON files from disk, merges them into a single `ResultsMap`. Handles:
- Missing files (returns empty)
- Empty files (returns empty)
- Parse errors (returns error)
- Merges both sources by theorem name and attempt key

#### `extract_model_data(data, model_name)` → `BTreeMap<String, BTreeMap<String, String>>`
Extracts per-theorem attempt maps from a flat JSON `{"<model>": {"<thm>": {"attempt_N": "text"}}}` structure. Handles missing fields gracefully.

---

## Layer 8: CLI Entry Point (`main.rs`)

Uses `clap` derive macros for argument parsing.

### Commands

| Command | Args | Description |
|---------|------|-------------|
| `list-models` | — | Prints all 6 model names |
| `generate` | `-m <model> -p data/models/<name> [-n 128] [--parallel 8] [--port 8080] [--run-id default]` | Runs the full pipeline for one model |
| `report` | `-m <model> [--run-id default]` | Placeholder — directs to output JSONs |
| `status` | `[--run-id default]` | Prints checkpoint progress for all models |

### `main()` flow for `generate` command:
1. Parse CLI args → resolve `ModelConfig` via `find_model()`
2. Build `PipelineConfig` with project_root=".", port, attempts, parallel
3. Print model info
4. `EvaluationPipeline::new(config, &run_id).run(&model_cfg, &model_path).await`
5. Print completion banner

---

## Data Flow: End-to-End

```
data/raw/minif2f.jsonl (488 theorems)
  │
  ▼
Theorem { name, split, header, informal_prefix, formal_statement, goal }
  │
  ├─► PromptBuilder::build(theorem)
  │     │
  │     ├─► build_user_message(theorem)   ← format-specific user prompt
  │     │     ├─ build_kimina()           → "Think about and solve..."
  │     │     ├─ build_goedel_v2()        → "Complete the following Lean 4 code:" + sorry
  │     │     ├─ build_simple()           → "Complete... with explanatory comments"
  │     │     └─ build_deepseek_prover()  → "Complete..." (no informal_prefix)
  │     │
  │     └─► Chat template wrap (qwen3 / deepseek_v2 / deepseek_coder / raw)
  │
  ▼
Full prompt string → JSON body {prompt, max_tokens, temperature, top_p, seed, stop}
  │
  ├─► buffer_unordered(concurrency) → HTTP POST /completion
  │     │
  │     └─► InferenceEngine::generate_one_with_retry() → model_output_text
  │
  ▼
Per-theorem batch (`completion_attempts` results)
  │
  ├─► rayon::par_iter(): parallel extraction
  │     │
  │     ├─► PromptBuilder::extract_proof(text) → proof_body
  │     │     ├─ find ```lean4 after </think>
  │     │     ├─ fallback: any ```lean4 block
  │     │     ├─ fallback: extract_lean_from_text()
  │     │     └─ last resort: strip everything, validate
  │     │
  │     ├─► Theorem::make_proof_file(proof_body) → assembled Lean code
  │     │
  │     └─► PromptBuilder::validate_lean_code(lean) → bool
  │           ├─ has ":= by"? no sorry?
  │           ├─ has tactics after ":= by"?
  │           ├─ no markdown/chat artefacts?
  │           ├─ is_proof_body()?
  │           └─ strip_block_comments() → ≥2 chars remain?
  │
  ▼
ResultsMap: { theorem → { attempt_N → (raw_output, lean_code) } }
  │
  ├─► write_flat_json() → output/raw_output/<model>.json   (pick raw)
  └─► write_flat_json() → output/lean_code/<model>.json    (pick lean, "" if invalid)
  │
  ▼
CheckpointManager::mark_done() → results/checkpoints/<model>__<run_id>.json
```

---

## Architecture: Continuous Request Pool

The key architectural decision: **NO per-theorem barrier**.

### Old architecture (abandoned):
```
for each theorem:
    submit N requests
    wait for ALL N ← BARRIER (GPU idle!)
    extract + validate (CPU work)
```
GPU utilization dropped to 4-8% between theorems.

### Current architecture:
```
All pending theorem × attempt requests are submitted through buffer_unordered(concurrency).
buffer_unordered keeps `concurrency` requests in flight at all times.
When one completes → next job starts immediately.
Results from ANY theorem flow in → batched per theorem → flushed when complete.
```
GPU stays at ~90%+ utilization. No idle gaps.

### Why `buffer_unordered` and not `FuturesUnordered`?
- `buffer_unordered(N)` is semantically equivalent to `FuturesUnordered` with backpressure
- It ensures at most N concurrent futures, preventing unbounded memory growth
- Jobs are ordered by theorem in the input stream, but results arrive in completion order

### Rayon parallel extraction
When a theorem's configured attempts are complete, `rayon::par_iter()` splits extraction+validation operations across all CPU cores. This keeps the main async loop free to continue feeding the GPU while CPU work happens in parallel.

---

## Prompt Formats Summary

| Format | Models | Input Type | `sorry` | Content |
|--------|--------|-----------|---------|---------|
| `kimina` | Kimina-RL-1.7B, Kimina-Distill-8B | Chat (system+user) | No | "Think about and solve..." with `# Problem:` and `# Formal statement:` |
| `goedel_v2` | Goedel-V2-8B | Chat (user only) | Yes | "Complete the following Lean 4 code:" + proof plan request (CoT) |
| `goedel_v2_nocot` | DeepSeek-Prover-V2-7B | Chat (user only) | Yes | "Complete the following Lean 4 code:" — no proof plan (non-CoT) |
| `simple` | Goedel-Prover-DPO | Completion (raw) | No | "Complete... with explanatory comments..." + open code block |
| `deepseek_prover` | STP | Completion (raw) | No | "Complete the following Lean 4 code:" + open code block (from `:= by`, no informal_prefix) |

---

## Stop Sequences by Model

| Model | Primary EOS | Additional Stops |
|-------|------------|------------------|
| Kimina-RL / Distill | `<\|im_end\|>` (151645) | `</s>` |
| Goedel-V2 | `<\|im_end\|>` (151645) | `</s>` |
| DeepSeek-Prover-V2 | `<｜end▁of▁sentence｜>` (100001) | `<｜Assistant｜>`, `<｜User｜>`, `</s>` |
| Goedel-Prover-DPO | `<｜end▁of▁sentence｜>` (100001) | `<\|EOT\|>`, `### Instruction:`, `</s>` |
| STP | `<｜end▁of▁sentence｜>` (100001) | `</s>` |

---

## Hardware Configuration

- **GPU**: RTX 5090 32GB (CUDA)
- **BF16 safetensors → FP8 at load time**: ~7-8 GB VRAM per 7-8B model, ~1.7 GB per 1.7B
- **KV cache**: vLLM PagedAttention — dynamically managed, not static slot allocation
- **Per-model parallelism** (current `scripts/generate-all.sh` values):
  - Kimina-Prover-Distill-8B: 48
  - STP_model_Lean: 64
  - Goedel-Prover-DPO: 40
  - DeepSeek-Prover-V2-7B: 32
  - Kimina-Prover-RL-1.7B: 64
  - Goedel-Prover-V2-8B: 16

---

## Context Size Formula

```
per_slot = (max_tokens + 4096).min(max_model_len)
# vLLM uses per_seq as --max-model-len directly; no ctx_size multiplication needed for PagedAttention
```

- `max_tokens + 4096`: output budget plus prompt/reasoning headroom per slot
- `.min(max_model_len)`: capped at the model's official context limit
- vLLM PagedAttention: KV cache is dynamically managed across sequences
- q8_0 KV cache + shared paged pool makes large ctx-sizes viable (VRAM grows ~65KB/token for 8B, ~49KB/token for 1.7B)

Per-slot context:
| Model | max_tok | max_model_len | per_slot |
|-------|---------|---------------|----------|
| kimina-prover-rl-1.7b | 8096 | 40960 | **12,192** |
| goedel-prover-v2-8b | 32768 | 40960 | **36,864** |
| deepseek-prover-v2-7b | 8192 | 65536 | **12,288** |
| kimina-prover-distill-8b | 8096 | 40960 | **12,192** |
| goedel-prover-dpo | 2048 | 4096 | **4,096** |
| stp-model-lean | 1024 | 1024 | **1,024** |

---

## Checkpointing & Crash Recovery

- Checkpoint file: `results/checkpoints/<model>__<run_id>.json` — a JSON array of theorem names
- **Atomic write**: temp file → rename (Unix atomic)
- **Trigger**: per-theorem (when all configured attempts complete)
- **Resume**: `--run-id <run_id>` → loads checkpoint, skips completed theorems
- **Incremental JSON write**: every 20 theorems, both output JSONs are written to disk (independent of checkpoint)
  - Checkpoint only records theorem names, not proof data
  - Without incremental writes, a crash loses all generated proofs for theorems since the last JSON write

---

## Build, Test, Quality Gates

```bash
cargo fmt --check          # formatting verification
cargo clippy -- -D warnings  # lint (0 warnings required)
cargo test                 # unit tests (~35 tests)
cargo build --release      # optimized build
```

---

## Scripts

### `./run` — Interactive menu (8 options)
### `./scripts/setup.sh` — One-time deployment script
### `./scripts/generate-all.sh` — Parallel generation via tmux

Runs all 6 configured models sequentially. Each model gets:
- vLLM on port 8080 via `uv run`
- Per-model `--parallel` value
- Retry loop (max 5 attempts with exponential backoff)
- Run ID: `v128-YYYYMMDD-v{N}-{model_name}`

---

## Module Dependency Graph

```
main.rs
  ├── config.rs (PipelineConfig)
  ├── models.rs (find_model, list_model_names)
  └── pipeline.rs (EvaluationPipeline)
        ├── data.rs (Theorem, load_all)
        ├── inference.rs (InferenceEngine)
        ├── prompts.rs (PromptBuilder)
        └── checkpoint.rs (CheckpointManager)

prompts.rs
  ├── config.rs (ModelConfig)
  └── data.rs (Theorem)

inference.rs
  └── config.rs (ModelConfig)

pipeline.rs
  ├── config.rs (ModelConfig, PipelineConfig)
  ├── data.rs (Theorem, load_all)
  ├── inference.rs (InferenceEngine)
  ├── prompts.rs (PromptBuilder)
  └── checkpoint.rs (CheckpointManager)
```

---

## Key Design Decisions

1. **Continuous request pool** over per-theorem barrier — maximizes GPU utilization
2. **Rayon parallel extraction** — CPU-bound proof extraction runs in parallel, async loop stays responsive
3. **Multi-strategy proof extraction** — handles 4 different model output formats gracefully
4. **`find` not `rfind`** for theorem header stripping — preserves nested `have ... := by` blocks
5. **`strip_block_comments` before validation** — rejects commentary-only "proofs" from models that explain instead of proving
6. **Incremental JSON writes** — crash resilience independent of checkpoint system
7. **Chat template prepopulation** — DeepSeek Coder gets `### Response:\n```lean4\n{code}` to keep it in code-generation mode
8. **Conditional system prompt** — Qwen3 template omits system block when `system_prompt` is empty (Goedel-V2 official behavior)
9. **No BOS in template** — vLLM/tokenizer adds it automatically, prevents double BOS
10. **Architecture-conditional decoder** — byte-fallback only applied to LLaMA tokenizer models (raw, deepseek_v2), Qwen3 passes through
11. **Incremental JSON writes** — every 20 theorems, write both JSONs to disk

---

## Future Architecture (Blueprint)

The following sections describe the target architecture after industrialization phases.

### Modular Source Layout

```
src/
├── cli.rs                  # CLI entry (from main.rs)
├── config/
│   ├── mod.rs              # Config loading (from YAML)
│   ├── model.rs            # ModelConfig (serde from YAML)
│   ├── pipeline.rs         # PipelineConfig
│   └── validation.rs       # Config validation rules
├── backend/
│   ├── mod.rs              # InferenceBackend trait
│   ├── vllm.rs             # VllmBackend: spawn vLLM, HTTP /v1/completions
│   └── hf_generate.rs      # HfGenerateBackend: spawn Python, stdin/stdout JSON
├── prompts/
│   ├── mod.rs              # PromptBuilder (from YAML templates)
│   └── extraction.rs       # extract_proof + validate_lean_code
├── pipeline/
│   ├── mod.rs              # EvaluationPipeline::run()
│   ├── checkpoint.rs       # CheckpointManager
│   └── flush.rs            # flush_batch (rayon extraction)
├── provenance/
│   ├── mod.rs              # Provenance struct
│   ├── collect.rs          # Gather: git, nvidia-smi, model info
│   └── write.rs            # Embed _metadata into output JSON
├── logging/
│   ├── mod.rs              # StructuredLogger
│   └── events.rs           # LogEvent enum (typed events)
├── errors.rs               # PipelineError enum hierarchy
└── data.rs                 # Theorem + dataset loading
```

### InferenceBackend Trait

```rust
#[async_trait]
pub trait InferenceBackend: Send + Sync {
    async fn start(&mut self) -> Result<(), PipelineError>;
    async fn health_check(&self) -> bool;
    async fn generate(&self, prompt: &str, params: &GenerationParams)
        -> Result<GenerationResult, PipelineError>;
    fn recommended_concurrency(&self) -> usize;
    fn architecture(&self) -> Architecture;
    async fn stop(self: Box<Self>) -> Result<(), PipelineError>;
}

pub enum Architecture { Qwen3, Llama }

pub struct GenerationResult {
    pub text: String,                    // Already encoding-corrected
    pub tokens_generated: u32,
    pub finish_reason: FinishReason,     // Stop | Length | Truncated
    pub generation_duration_ms: u64,
}
```

### Error Hierarchy

```
PipelineError
├── Environment    (GpuNotFound, PortInUse, ModelFileMissing) → no retry
├── Transient      (NetworkTimeout, VllmBusy)                  → retry 3x
├── DataError      (EmptyOutput, EncodingCorrupt)              → skip attempt
└── ModelError     (VllmStartFailed, AllEmpty)                 → skip model
```

### Validation Pipeline (per model)

```
CHECK-1: Structure  — JSON valid, 488 theorems, 128 attempts each
CHECK-2: Encoding   — per-architecture thresholds (Qwen3: 0 U+FFFD, Llama: <1%)
CHECK-3: Quality    — non-empty rate, extraction rate, duplication rate
CHECK-4: Sampling   — 25 samples checked for basic Lean validity
CHECK-5: Consistency — diff vs previous run of same model
Result:  PASS (continue) | WARN (record + continue) | ERROR (pause + ask) | FATAL (stop)
```

### Run Lifecycle

```
1. INIT       → run_id, manifest.partial.json, structured log
2. PREFLIGHT  → GPU free, port free, model exists, config valid
3. BACKEND    → start vLLM/HF, health check
4. GENERATE   → buffer_unordered loop, checkpoint, incremental write
5. BACKEND    → stop, free GPU
6. VERIFY     → 5-level check, report generation
7. PROVENANCE → finalize manifest, embed _metadata
8. NEXT       → loop to step 2 for next model, or COMPLETE

CRASH   → manifest.partial preserved, checkpoint preserved
RESUME  → load manifest + checkpoint, continue from breakpoint
```

### Output Schema (v2)

```json
{
  "_metadata": { /* provenance — self-describing */ },
  "models": {
    "<model>": {
      "<theorem>": {
        "attempt_1": {
          "raw": "...",
          "lean": "...",
          "finish_reason": "stop"
        }
      }
    }
  }
}
```

### Config YAML Format (configs/models/<name>.yaml)

```yaml
name: goedel-prover-dpo
hf_repo: Goedel-LM/Goedel-Prover-DPO
backend: { type: vllm, quantization: fp8 }
architecture: { type: raw, tokenizer: llama }
prompt: { format: simple, include_sorry: false, code_block: open }
inference: { max_model_len: 4096, max_tokens: 2048, temperature: 1.0, top_p: 0.95, seed: 1 }
validation: { u_fffd_max_rate: 0.001, min_extraction_rate: 0.70 }
sources: { hf_url: "...", paper: "...", github: "..." }
```
10. **Atomic checkpoint writes** — temp file + rename for crash-safe persistence
