#!/usr/bin/env python3
"""STP_model_Lean independent runner using HF model.generate().

Uses native BF16 precision and model.generate() which respects
begin_suppress_tokens from config.json — preventing empty outputs
that vLLM produces due to lack of EOS suppression.

Output format matches the Rust pipeline exactly:
  output/raw_output/stp-model-lean.json
  output/lean_code/stp-model-lean.json

Usage:
  python scripts/stp_runner.py [--attempts 128] [--batch 4] [--skip N]
"""

import argparse
import json
import os
import re
import sys
import time
from pathlib import Path

import torch
from transformers import AutoModelForCausalLM, AutoTokenizer
from tqdm import tqdm

PROJECT_ROOT = Path(__file__).resolve().parent.parent
MODEL_PATH = PROJECT_ROOT / "data" / "models" / "stp-model-lean"
DATA_PATH = PROJECT_ROOT / "data" / "raw" / "minif2f.jsonl"
RAW_OUTPUT = PROJECT_ROOT / "output" / "raw_output" / "stp-model-lean.json"
LEAN_OUTPUT = PROJECT_ROOT / "output" / "lean_code" / "stp-model-lean.json"
CHECKPOINT_FILE = PROJECT_ROOT / "results" / "checkpoints" / "stp-model-lean__stp-hf.json"


# ── GPT-2 ByteLevel decoder (matching Rust decode_llama_byte_fallback) ──
# LlamaTokenizer (slow) does NOT perform ByteLevel decoding — byte-encoded
# tokens (Ġ=0x20=space, Ċ=0x0A=newline, etc.) remain as their Unicode
# representations.  This reverses them via the canonical GPT-2
# bytes_to_unicode inverse table, same as inference.rs: gpt2_unicode_to_byte.

def _build_gpt2_byte_table():
    bs = list(range(ord("!"), ord("~") + 1)) + list(range(ord("¡"), ord("¬") + 1)) + list(range(ord("®"), ord("ÿ") + 1))
    cs = bs[:]; n = 0
    for b in range(256):
        if b not in bs:
            bs.append(b); cs.append(256 + n); n += 1
    return {chr(c): b for b, c in zip(bs, cs)}

_GPT2_BYTE_TABLE = _build_gpt2_byte_table()

def decode_byte_fallback(text: str) -> str:
    out = bytearray()
    for ch in text:
        if ch in _GPT2_BYTE_TABLE:
            out.append(_GPT2_BYTE_TABLE[ch])
        else:
            out.extend(ch.encode("utf-8"))
    return out.decode("utf-8", errors="replace")

# ── Prompt building (matching Rust build_deepseek_prover) ──────────


def theorem_block(theorem: dict) -> str:
    parts = [theorem.get("header", "")]
    # informal_prefix intentionally EXCLUDED (STP has 1024 ctx)
    stmt = theorem["formal_statement"]
    if "sorry" in stmt:
        stmt = stmt.rsplit("sorry", 1)[0].strip()
    parts.append(stmt)
    return "\n".join(p for p in parts if p)


def build_prompt(theorem: dict) -> str:
    block = theorem_block(theorem)
    return f"Complete the following Lean 4 code:\n\n```lean4\n{block}"


# ── Proof extraction (matching Rust extract_proof + validate_lean_code) ──


def extract_fenced_code(text: str) -> str | None:
    """Find ```lean4 block with most content after stripping theorem header."""
    best, best_len = None, 0
    for fence in ["```lean4\n", "```lean4", "```tactics\n", "```tactics", "```\n", "```"]:
        search_from = 0
        is_bare = fence in ("```", "```\n")
        while True:
            pos = text.find(fence, search_from)
            if pos == -1:
                break
            start = pos + len(fence)
            end = text.find("```", start)
            if end == -1:
                break
            code = text[start:end].strip()
            if is_bare:
                # Strip language specifier from first line
                nl = code.find("\n")
                if nl != -1:
                    first = code[:nl].strip()
                    if first and " " not in first and "/" not in first and all(c.isalnum() or c in "_-" for c in first):
                        code = code[nl + 1 :].strip()
            stripped = strip_theorem_header(code)
            if len(stripped.strip()) > best_len:
                best_len = len(stripped.strip())
                best = code
            search_from = end + 3
    return best


def strip_theorem_header(code: str) -> str:
    """Strip everything before := by, preserving nested have blocks."""
    pos = code.find(":= by\n")
    if pos != -1:
        after = code[pos + len(":= by\n") :]
        if is_proof_body(after.strip()):
            return after.strip()
    pos = code.find(":= by")
    if pos != -1:
        after_pos = pos + len(":=")
        rest = code[after_pos:].strip()
        after_by = rest.removeprefix("by").strip()
        if after_by and is_proof_body(after_by):
            return after_by
    pos = code.find(":=by")
    if pos != -1:
        after_by = code[pos + 4 :].strip()
        if after_by and is_proof_body(after_by):
            return after_by
    return code


def is_proof_body(text: str) -> bool:
    """Check if text looks like Lean tactics, not natural language."""
    t = text.strip()
    if not t:
        return False
    first = t.split()[0]
    if first in ("theorem", "lemma", "import", "open", "set_option", "noncomputable", "variable"):
        return False
    if t.startswith("```") or t.startswith("#"):
        return False
    if first[0].isupper() and len(t.split()) > 4 and not t.startswith("--") and not t.startswith("/-"):
        return False
    return True


def has_proof_body(code: str) -> bool:
    stripped = strip_theorem_header(code).strip()
    if len(stripped) < 2 or stripped.startswith("`"):
        return False
    if "sorry" in stripped:
        return False
    return is_proof_body(stripped)


def strip_block_comments(text: str) -> str:
    """Remove Lean /- ... -/ block comments (handles nesting)."""
    result = []
    i = 0
    while i < len(text):
        if text[i : i + 2] == "/-":
            depth = 1
            i += 2
            while i < len(text) and depth > 0:
                if text[i : i + 2] == "/-":
                    depth += 1
                    i += 2
                elif text[i : i + 2] == "-/":
                    depth -= 1
                    i += 2
                else:
                    i += 1
        else:
            result.append(text[i])
            i += 1
    return "".join(result)


def validate_lean_code(code: str) -> bool:
    if not code:
        return False
    if ":= by" not in code:
        return False
    if "sorry" in code:
        return False
    pos = code.find(":= by")
    after = code[pos + len(":= by") :].strip()
    if len(after) < 2:
        return False
    if "```" in after or "**" in after:
        return False
    for tok in ("<|im_start|>", "<|im_end|>", "<｜User｜>", "<｜Assistant｜>"):
        if tok in after:
            return False
    if not is_proof_body(after):
        return False
    without = strip_block_comments(after)
    if len(without.strip()) < 2:
        return False
    return True


def extract_proof(raw: str) -> str:
    """Multi-strategy proof extraction matching Rust implementation."""
    # Strategy 1: ```lean4 block after </think>
    search_from = raw.find("</think>")
    if search_from != -1:
        search_from += len("</think>")
        code = extract_fenced_code(raw[search_from:])
        if code and has_proof_body(code):
            return code

    # Strategy 2: any ```lean4 block in entire text
    code = extract_fenced_code(raw)
    if code and has_proof_body(code):
        return code

    # Strategy 3: extract tactics from raw text
    pos = raw.find(":= by")
    if pos != -1:
        after = raw[pos + len(":= by") :]
        lines = after.split("\n")
        start_idx = 0
        while start_idx < len(lines) and not lines[start_idx].strip():
            start_idx += 1
        if start_idx < len(lines):
            tactics = []
            for line in lines[start_idx:]:
                trimmed = line.strip()
                if not trimmed:
                    break
                if trimmed.startswith(("theorem ", "lemma ", "import ")):
                    break
                if line.startswith((" ", "\t")) or trimmed.startswith(("·", ".", "--")):
                    tactics.append(line)
                elif not tactics:
                    tactics.append(line)
                else:
                    break
            combined = "\n".join(tactics).strip()
            if combined and has_proof_body(combined):
                return combined

    return ""


def make_proof_file(theorem: dict, proof_body: str) -> str:
    parts = []
    for key in ("header", "informal_prefix"):
        if theorem.get(key):
            parts.append(theorem[key])
    parts.append(theorem["formal_statement"])
    parts.append(proof_body)
    return "\n".join(p for p in parts if p)


def save_json(data: dict, path: Path) -> None:
    os.makedirs(path.parent, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    with open(tmp, "w") as f:
        json.dump(data, f)
    os.replace(tmp, path)


def load_checkpoint() -> set:
    if CHECKPOINT_FILE.exists():
        try:
            with open(CHECKPOINT_FILE) as f:
                return set(json.load(f))
        except (json.JSONDecodeError, TypeError):
            pass
    return set()


def save_checkpoint(done: set) -> None:
    os.makedirs(CHECKPOINT_FILE.parent, exist_ok=True)
    tmp = CHECKPOINT_FILE.with_suffix(".tmp")
    with open(tmp, "w") as f:
        json.dump(sorted(done), f)
    os.replace(tmp, CHECKPOINT_FILE)


# ── Main ────────────────────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(description="STP model HF generate runner")
    parser.add_argument("--attempts", type=int, default=128)
    parser.add_argument("--batch", type=int, default=4, help="Batch size for generate()")
    parser.add_argument("--skip", type=int, default=0, help="Skip first N theorems")
    args = parser.parse_args()

    print("=" * 60)
    print("STP_model_Lean — HF model.generate() runner")
    print(f"Attempts: {args.attempts}, Batch: {args.batch}")
    print("=" * 60)

    # Load model
    print("\nLoading model (BF16 native)...")
    model = AutoModelForCausalLM.from_pretrained(
        str(MODEL_PATH),
        torch_dtype=torch.bfloat16,
        trust_remote_code=True,
    ).cuda()
    model.eval()
    tokenizer = AutoTokenizer.from_pretrained(str(MODEL_PATH), trust_remote_code=True)
    print(f"  Model: {sum(p.numel() for p in model.parameters()):,} params")
    print(f"  Tokenizer: {tokenizer.__class__.__name__}")
    print(f"  begin_suppress_tokens: {getattr(model.config, 'begin_suppress_tokens', None)}")

    # Load theorems
    theorems = []
    with open(DATA_PATH) as f:
        for line in f:
            if line.strip():
                theorems.append(json.loads(line))
    print(f"\nTheorems: {len(theorems)}")

    if args.skip:
        theorems = theorems[args.skip:]
        print(f"  Skipping first {args.skip}, {len(theorems)} remaining")

    # Load checkpoint
    done = load_checkpoint()
    if done:
        print(f"  Checkpoint: {len(done)} already done, resuming")
    pending = [t for t in theorems if t["name"] not in done]
    print(f"  Pending: {len(pending)}")

    if not pending:
        print("\nAll done!")
        return

    # Load existing outputs for resume
    raw_data = {}
    lean_data = {}
    if RAW_OUTPUT.exists():
        with open(RAW_OUTPUT) as f:
            raw_data = json.load(f)
    if LEAN_OUTPUT.exists():
        with open(LEAN_OUTPUT) as f:
            lean_data = json.load(f)

    model_key = "stp-model-lean"
    raw_thms = raw_data.get(model_key, {})
    lean_thms = lean_data.get(model_key, {})

    total = len(pending) * args.attempts
    pbar = tqdm(total=total, desc="Generating", unit="req")

    # Generate
    for theorem in pending:
        name = theorem["name"]
        prompt = build_prompt(theorem)

        raw_attempts = {}
        lean_attempts = {}

        # Batch generate
        for batch_start in range(0, args.attempts, args.batch):
            batch_size = min(args.batch, args.attempts - batch_start)
            prompts = [prompt] * batch_size

            inputs = tokenizer(prompts, return_tensors="pt", padding=True).to("cuda")
            # Per-batch deterministic seed for reproducibility. The Rust pipeline
            # uses base_seed.wrapping_add(attempt_index); mirror that here by
            # seeding from the first attempt index in this batch (seed base = 1).
            torch.manual_seed(1 + batch_start)
            with torch.no_grad():
                outputs = model.generate(
                    **inputs,
                    max_new_tokens=1024,
                    temperature=1.0,
                    top_p=1.0,
                    do_sample=True,
                )

            for i in range(batch_size):
                attempt_idx = batch_start + i
                att_key = f"attempt_{attempt_idx + 1}"

                # Decode: strip the prompt tokens
                output_ids = outputs[i][inputs["input_ids"].shape[1] :]
                raw = tokenizer.decode(output_ids, skip_special_tokens=False)

                # Clean up special tokens
                raw = raw.replace("<｜end▁of▁sentence｜>", "")
                raw = raw.replace("<｜begin▁of▁sentence｜>", "")
                raw = raw.replace("</s>", "")
                raw = raw.replace("<｜end of sentence｜>", "")  # halfwidth variant
                while "[PAD]" in raw:
                    raw = raw.replace("[PAD]", "")
                # GPT-2 ByteLevel decoder: LlamaTokenizer (slow) does NOT perform
                # ByteLevel decoding, so byte-encoded tokens (Ġ=space, Ċ=newline,
                # etc.) remain as Unicode chars.  Apply the same inverse mapping
                # used in the Rust pipeline (inference.rs: gpt2_unicode_to_byte).
                raw = decode_byte_fallback(raw)
                raw_attempts[att_key] = raw

                # Extract proof
                proof = extract_proof(raw)
                if proof:
                    lean = (
                        proof
                        if "import " in proof
                        else make_proof_file(theorem, proof)
                    )
                    if not validate_lean_code(lean):
                        lean = ""
                else:
                    lean = ""
                lean_attempts[att_key] = lean

                pbar.update(1)

        # Save per theorem
        raw_thms[name] = raw_attempts
        lean_thms[name] = lean_attempts

        done.add(name)
        save_checkpoint(done)

        # Incremental JSON write every 5 theorems
        if len(done) % 5 == 0:
            save_json({model_key: raw_thms}, RAW_OUTPUT)
            save_json({model_key: lean_thms}, LEAN_OUTPUT)

    pbar.close()

    # Final save
    save_json({model_key: raw_thms}, RAW_OUTPUT)
    save_json({model_key: lean_thms}, LEAN_OUTPUT)

    # Stats
    total_lean = sum(1 for t in lean_thms.values() for v in t.values() if v)
    total_attempts = sum(len(t) for t in lean_thms.values())
    rate = total_lean / total_attempts * 100 if total_attempts else 0
    print(f"\nDone: {len(done)} theorems, {total_attempts} attempts")
    print(f"Extraction rate: {total_lean}/{total_attempts} ({rate:.1f}%)")


if __name__ == "__main__":
    main()
