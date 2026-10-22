#!/usr/bin/env python3
"""Feedback-driven search of 24 proved compactor strategies, not an LLM.

The current parent's settings are changed one dimension at a time where possible.
Every combination is generated with its proofs and checked by Temper. A future
model proposer can replace this worker without changing the verification loop.
"""
import itertools
import json
import os
from pathlib import Path
import subprocess
import sys

PREFIX = "-- raw-search-config "
DEFAULT = {"dictionary": "ascending", "cache": "tree", "codec": "list"}
GRID = list(itertools.product(["ascending", "descending", "short-first", "long-first"],
                             ["tree", "hash"], ["list", "bytes", "stream"]))

BYTE_CODEC = r'''
def joinParts (parts : List Contract.Bytes) : ByteArray :=
  parts.foldl (fun acc part => acc ++ part.toByteArray) ByteArray.empty

theorem foldParts_eq (parts : List Contract.Bytes) (acc : ByteArray) :
    parts.foldl (fun acc part => acc ++ part.toByteArray) acc = acc ++ parts.flatten.toByteArray := by
  induction parts generalizing acc with
  | nil => simp
  | cons part rest ih =>
    simp only [List.foldl_cons, List.flatten_cons, List.toByteArray_append]
    rw [ih, ByteArray.append_assoc]

theorem joinParts_eq (parts : List Contract.Bytes) : joinParts parts = parts.flatten.toByteArray := by
  simp [joinParts, foldParts_eq]

def encoded (out : Contract.Output) : ByteArray :=
  let index := indexes out.dictionary 0
  let data := joinParts (out.rows.map (rowBytes index))
  let dict := joinParts (out.dictionary.map (fun tag => tag.toUTF8.data.toList ++ [10]))
  let size := data.size + dict.size
  ([1, size, size, out.rows.length, out.dictionary.length, data.size].flatMap Contract.varint).toByteArray ++ data ++ dict

theorem encoded_eq (out : Contract.Output) : encoded out = (rawBytes out).toByteArray := by
  simp only [encoded, rawBytes, joinParts_eq, ← List.flatMap_def,
    List.size_toByteArray, List.toByteArray_append]
'''

STREAM_CODEC = r'''
theorem push_eq (items : Contract.Bytes) (acc : ByteArray) :
    items.foldl ByteArray.push acc = acc ++ items.toByteArray := by
  induction items generalizing acc with
  | nil => simp
  | cons byte rest ih =>
    rw [List.foldl_cons, ih]
    change acc.push byte ++ rest.toByteArray = acc ++ ([byte] ++ rest).toByteArray
    rw [List.toByteArray_append, ← ByteArray.append_assoc, ByteArray.append_toByteArray_singleton]

theorem foldWrite_eq (items : List α) (write : ByteArray → α → ByteArray)
    (bytes : α → Contract.Bytes)
    (h : ∀ acc item, write acc item = acc ++ (bytes item).toByteArray) (acc : ByteArray) :
    items.foldl write acc = acc ++ (items.flatMap bytes).toByteArray := by
  induction items generalizing acc with
  | nil => simp
  | cons item rest ih =>
    simp only [List.foldl_cons, List.flatMap_cons, List.toByteArray_append]
    rw [ih, h, ByteArray.append_assoc]

def writeRow (index : Std.TreeMap String Nat) (acc : ByteArray) (row : Contract.Row) : ByteArray :=
  let fields := Contract.littleEndian 4 row.org.toNat ++ Contract.littleEndian 8 row.metric.toNat ++
    Contract.littleEndian 4 row.timestamp.toNat ++ Contract.littleEndian 8 row.valueBits.toNat ++
    Contract.varint row.tags.length
  row.tags.foldl (fun out tag => (Contract.varint (index.getD tag 0)).foldl ByteArray.push out)
    (fields.foldl ByteArray.push acc)

theorem writeRow_eq (index : Std.TreeMap String Nat) (acc : ByteArray) (row : Contract.Row) :
    writeRow index acc row = acc ++ (rowBytes index row).toByteArray := by
  unfold writeRow
  rw [foldWrite_eq _ _ (fun tag => Contract.varint (index.getD tag 0)) (fun a _ => push_eq _ a)]
  simp only [push_eq, rowBytes, List.toByteArray_append, ByteArray.append_assoc]

theorem byteRoundtrip (bytes : ByteArray) : bytes.data.toList.toByteArray = bytes := by
  apply ByteArray.ext
  simp [List.data_toByteArray]

def writeTag (acc : ByteArray) (tag : String) : ByteArray := (acc ++ tag.toUTF8).push 10

theorem writeTag_eq (acc : ByteArray) (tag : String) :
    writeTag acc tag = acc ++ (tag.toUTF8.data.toList ++ [10]).toByteArray := by
  unfold writeTag
  rw [List.toByteArray_append, byteRoundtrip, ← ByteArray.append_assoc,
    ByteArray.append_toByteArray_singleton]

def encoded (out : Contract.Output) : ByteArray :=
  let index := indexes out.dictionary 0
  let data := out.rows.foldl (writeRow index) ByteArray.empty
  let dict := out.dictionary.foldl writeTag ByteArray.empty
  let size := data.size + dict.size
  ([1, size, size, out.rows.length, out.dictionary.length, data.size].flatMap Contract.varint).toByteArray ++ data ++ dict

theorem encoded_eq (out : Contract.Output) : encoded out = (rawBytes out).toByteArray := by
  dsimp only [encoded]
  rw [foldWrite_eq _ _ (rowBytes _) (writeRow_eq _)]
  rw [foldWrite_eq _ _ (fun tag => tag.toUTF8.data.toList ++ [10]) writeTag_eq]
  simp only [ByteArray.empty_append, List.size_toByteArray, rawBytes, List.toByteArray_append]
'''


def configuration(source):
    first = source.splitlines()[0] if source else ""
    return json.loads(first[len(PREFIX):]) if first.startswith(PREFIX) else DEFAULT.copy()


def key(config):
    return tuple(config[field] for field in DEFAULT)


def generate(config):
    source = (Path(__file__).resolve().parent / "Baseline.lean").read_text()
    # All dictionary orders preserve the exact set of tags and uniqueness.
    if config["dictionary"] != "ascending":
        source = source.replace("def dictionary (", "def dictionaryBase (")
        source = source.replace("dictionary rows", "dictionaryBase rows")
        source = source.replace("theorem dictionary_unique", "theorem dictionaryBase_unique")
        source = source.replace("theorem dictionary_contents", "theorem dictionaryBase_contents")
        source = source.replace("simp [dictionary,", "simp [dictionaryBase,")
        if config["dictionary"] == "descending":
            expression = "(dictionaryBase rows).reverse"
            unique = "by\n    apply List.pairwise_reverse.mpr\n    exact (dictionaryBase_unique rows).imp (fun h => Ne.symm h)"
            contents = "by simpa [dictionary] using dictionaryBase_contents rows tag"
        else:
            relation = "≤" if config["dictionary"] == "short-first" else "≥"
            expression = "(dictionaryBase rows).mergeSort (fun a b => decide (a.length %s b.length))" % relation
            unique = "(List.mergeSort_perm _ _).nodup_iff.mpr (dictionaryBase_unique rows)"
            contents = "(List.mergeSort_perm _ _).mem_iff.trans (dictionaryBase_contents rows tag)"
        start = source.index("def builder")
        source = source[:start] + ("def dictionary (rows : List Contract.Row) : List String :=\n  " + expression + "\n\n"
            + "theorem dictionary_unique (rows : List Contract.Row) : (dictionary rows).Nodup :=\n  " + unique + "\n\n"
            + "theorem dictionary_contents (rows : List Contract.Row) (tag : String) :\n"
            + "    tag ∈ dictionary rows ↔ tag ∈ Contract.Tags rows :=\n  " + contents + "\n\n") + source[start:]
    if config["codec"] != "list":
        index = source.index("def render")
        source = source[:index] + (BYTE_CODEC if config["codec"] == "bytes" else STREAM_CODEC) + "\n" + source[index:]
        source = source.replace("(outputs.map (fun out => (rawBytes out).toByteArray)).toArray",
                                "(outputs.map encoded).toArray")
        source = source.replace("  rw [rawBytes_eq]\n", "  rw [encoded_eq, rawBytes_eq]\n")
    if config["cache"] == "hash":
        source = source.replace("Std.TreeMap", "Std.HashMap")
    return PREFIX + json.dumps(config, sort_keys=True) + "\n" + source


def propose(context):
    parent = configuration((context.get("parent") or {}).get("source", ""))
    seen = {key(DEFAULT)} if context.get("parent") else set()
    recent = context.get("recent_proposals", [])
    for attempt in context.get("history", []):
        header = attempt.get("proposal_header") or ""
        if header.startswith(PREFIX):
            config = json.loads(header[len(PREFIX):])
            already_verified = any(a.get("proposal_header") == header and a.get("candidate")
                                   for a in context.get("history", []))
            if attempt.get("candidate") is None and not already_verified:
                for failure in recent:
                    old_source = failure.get("source") or ""
                    if old_source.startswith(header + "\n"):
                        if generate(config) != old_source:
                            return generate(config)  # Retry a repaired proof, not a changed spec.
                        break
            seen.add(key(config))
    remaining = [item for item in GRID if item not in seen]
    if not remaining:
        return ""
    # Prefer a new single-setting mutation of the current verified parent. The
    # engine may choose another archived parent when the best branch stalls.
    target = min(remaining, key=lambda item: (sum(a != b for a, b in zip(item, key(parent))), GRID.index(item)))
    return generate(dict(zip(DEFAULT, target)))


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "--check":
        environment = {**os.environ, "LEAN_PATH": sys.argv[3]}
        for item in GRID:
            source = generate(dict(zip(DEFAULT, item)))
            result = subprocess.run([sys.argv[2], "--stdin"], input=source, text=True,
                                    capture_output=True, env=environment)
            if result.returncode:
                raise RuntimeError(str(item) + "\n" + result.stdout + result.stderr)
        print(json.dumps({"complete_proof_templates": len(GRID)}))
    elif len(sys.argv) == 3 and sys.argv[1] == "--configuration":
        print(generate(json.loads(sys.argv[2])), end="")
    else:
        print(propose(json.loads(Path(sys.argv[1]).read_text())), end="")
