#!/usr/bin/env python3
"""Runnable model-backed proposer adapter; no model is implicitly selected.

TEMPER_LEAN_MODEL_COMMAND is a JSON argv array for an inference command accepting
a prompt on stdin and emitting its final response on stdout. Temper supplies the
immutable contract, current parent, rejected source/proof errors and measurements.
This worker makes a real command invocation; it never substitutes canned source.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def prompt(context):
    return """Generate one substantially improved, COMPLETE Lean implementation with all proofs.
Return only the complete Candidate.lean source (import Spec, namespace Candidate).
Do not alter Spec, obligations, benchmark data or metric. Do not use sorry, admit,
unsafe/partial definitions, axioms, custom attributes, macros, IO or native_decide.
Optimize real CPU work and allocations, not benchmark recognition or special cases.
If a recent proof failed, repair it before expanding that proposal. If improvements
stall, change algorithm/representation and explore a different strategy. Never
stop because the first improvement succeeded. Avoid identical or cosmetic proposals.
The host will check every proof, compile, benchmark and return feedback. Performance
scores are empirical; all original semantic/format obligations remain mandatory.
Consider streaming byte encoding, input parsing, sort/merge strategies, dictionary
construction, allocation reuse, and index representation. Preserve all valid inputs.
Do not return a plan or an explanation; return a proved implementation.

Frozen contract and actual search feedback:
""" + json.dumps(context, ensure_ascii=False)


def propose(context, command):
    if not isinstance(command, list) or not command or not all(isinstance(s, str) and s for s in command):
        raise ValueError("TEMPER_LEAN_MODEL_COMMAND must be a nonempty JSON argv array")
    result = subprocess.run(command, input=prompt(context), text=True, capture_output=True,
                            timeout=max(1, int(context.get("generation_seconds", 900)) - 5))
    if result.returncode:
        raise RuntimeError("model command failed (%s): %s" % (result.returncode, result.stderr[-2000:]))
    source = result.stdout.strip()
    blocks = re.findall(r"```(?:lean|lean4)?\s*\n(.*?)```", source, re.DOTALL)
    if blocks:
        source = next((block for block in reversed(blocks) if "import Spec" in block), blocks[-1]).strip()
    return source


if __name__ == "__main__":
    configured = os.environ.get("TEMPER_LEAN_MODEL_COMMAND")
    if not configured:
        raise SystemExit("Configure TEMPER_LEAN_MODEL_COMMAND explicitly; no model or credentials are selected implicitly")
    context = json.loads(Path(sys.argv[1]).read_text())
    print(propose(context, json.loads(configured)), end="")
