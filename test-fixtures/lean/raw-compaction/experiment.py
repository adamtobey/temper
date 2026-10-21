#!/usr/bin/env python3
"""Reproducible data/independent RAW oracle for the pure Lean example.

This is experiment tooling, not an alternative app implementation. No third-party
Python packages are required. Generated contracts/artifacts go outside the source
package, so the specification and benchmark can be frozen before synthesis.
"""

import argparse
import collections
import ctypes
import hashlib
import json
from pathlib import Path
import random
import struct
import subprocess
import sys


PACKAGE = Path(__file__).resolve().parent
TOOLCHAIN = "leanprover/lean4:v4.30.0-rc2"


def varint(n):
    result = bytearray()
    while n >= 128:
        result.append((n & 127) | 128)
        n >>= 7
    result.append(n)
    return bytes(result)


def encode(rows):
    # Input dictionaries deliberately differ from either candidate's ordering.
    dictionary = list(dict.fromkeys(tag for row in rows for tag in row[4]))
    ids = {tag: i for i, tag in enumerate(dictionary)}
    payload = b"".join(
        struct.pack("<IQIQ", *row[:4])
        + varint(len(row[4]))
        + b"".join(varint(ids[tag]) for tag in row[4])
        for row in rows
    )
    dictionary_bytes = b"".join(tag.encode("utf-8") + b"\n" for tag in dictionary)
    size = len(payload) + len(dictionary_bytes)
    return b"".join(map(varint, [1, size, size, len(rows), len(dictionary), len(payload)])) + payload + dictionary_bytes


def decode(data):
    """Independent strict byte oracle; does not call Contract.readRaw."""
    position = 0

    def integer():
        nonlocal position
        start = position
        value = 0
        for shift in range(0, 70, 7):
            assert position < len(data), "truncated varint"
            byte = data[position]
            position += 1
            value |= (byte & 127) << shift
            if byte < 128:
                assert value < 2**64 and data[start:position] == varint(value), "noncanonical varint"
                return value
        raise AssertionError("oversized varint")

    version, compressed, uncompressed, count, entries, offset = [integer() for _ in range(6)]
    payload_start = position
    assert version == 1 and compressed == uncompressed == len(data) - position
    assert 0 <= offset <= compressed
    dictionary_start = payload_start + offset
    dictionary_blob = data[dictionary_start:]
    if entries:
        assert dictionary_blob.endswith(b"\n")
        dictionary = [tag.decode("utf-8", errors="strict") for tag in dictionary_blob[:-1].split(b"\n")]
    else:
        assert dictionary_blob == b""
        dictionary = []
    assert len(dictionary) == entries == len(set(dictionary)), "dictionary counts/uniqueness"
    rows = []
    for _ in range(count):
        assert position + 24 <= dictionary_start
        fields = struct.unpack_from("<IQIQ", data, position)
        position += 24
        tag_count = integer()
        tags = []
        for _ in range(tag_count):
            index = integer()
            assert index < entries, "tag reference out of bounds"
            tags.append(dictionary[index])
        rows.append((*fields, tuple(tags)))
    assert position == dictionary_start, "row/dictionary offset mismatch"
    return rows


def synthetic(seed, count, file_count):
    rng = random.Random(seed)
    inputs = [[] for _ in range(file_count)]
    # Include NaN payloads, infinities, negative zero and full-width identifiers.
    bits = [0, 0x8000000000000000, 0x7FF8000000001234, 0x7FF0000000000000,
            0xFFF0000000000000, 0x3FF4000000000000]
    previous = None
    for i in range(count):
        row = (rng.randrange(8), rng.randrange(max(1, count // 8)),
               (1_700_000_000 + i) & 0xFFFFFFFF, bits[i % len(bits)],
               ("env:prod", "host:%06d" % i, "region:%02d" % (i % 32), "unicode:東京☃"))
        if i % 11 == 0 and previous is not None:
            row = previous  # Preserve exact duplicates, not only sets.
        previous = row
        inputs[i % file_count].append(row)
    return [sorted(rows, key=lambda row: row[:2]) for rows in inputs]


def envelope(inputs, output_count):
    return "\n".join([str(output_count), *(encode(rows).hex() for rows in inputs)])


def check_response(request, response):
    count, *files = request.split("\n")
    input_rows = [row for file in files for row in decode(bytes.fromhex(file))]
    marker, *files = response.split("\n")
    assert marker == "ok" and len(files) == int(count), "success/output count"
    chunks = [decode(bytes.fromhex(file)) for file in files]
    output_rows = [row for chunk in chunks for row in chunk]
    assert collections.Counter(input_rows) == collections.Counter(output_rows), "row multiplicities/float bits/tags"
    keys = [row[:2] for row in output_rows]
    assert keys == sorted(keys), "global range ordering"
    remaining = len(input_rows)
    for index, chunk in enumerate(chunks):
        expected = (remaining + int(count) - index - 1) // (int(count) - index)
        assert len(chunk) == expected, "balanced partition"
        remaining -= len(chunk)
    return len(output_rows)


def prepare(directory):
    directory.mkdir(parents=True, exist_ok=False)
    (directory / "Spec.lean").write_bytes((PACKAGE / "Spec.lean").read_bytes())
    benchmark = json.loads((PACKAGE / "benchmark.json").read_text())
    inputs = synthetic(benchmark["seed"], benchmark["rows"], benchmark["input_files"])
    workload = envelope(inputs, benchmark["output_files"])
    manifest = {
        "version": 1, "tenant": "default", "component": "raw-compaction",
        "toolchain": TOOLCHAIN,
        "specification_proofs": ["consistent", "run_correct", "row_set_equality", "format_header", "tagIndex_resolves"],
        "obligations": ["correct"],
        "objective": {"metric": benchmark["metric"], "direction": benchmark["direction"], "inputs": [workload],
                      "samples": benchmark["samples"], "iterations": benchmark["iterations"],
                      "minimum_improvement": benchmark["minimum_improvement"]},
    }
    (directory / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    metadata = {"seed": benchmark["seed"], "rows": benchmark["rows"],
                "input_files": benchmark["input_files"], "output_files": benchmark["output_files"],
                "input_raw_bytes": sum(len(encode(rows)) for rows in inputs),
                "request_utf8_bytes": len(workload.encode()),
                "workload_sha256": hashlib.sha256(workload.encode()).hexdigest(),
                "spec_sha256": hashlib.sha256((directory / "Spec.lean").read_bytes()).hexdigest(),
                "manifest_sha256": hashlib.sha256((directory / "manifest.json").read_bytes()).hexdigest(),
                "python": sys.version.split()[0]}
    (directory / "workload.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(json.dumps(metadata, indent=2))


def test_artifact(directory):
    """Local test-only execution, not deployment or a grant of deployment review."""
    receipt = json.loads((directory / "receipt.json").read_text())
    native = (directory / "core.native").read_bytes()
    artifact_hash = hashlib.sha256(struct.pack("<Q", len(native)) + native).hexdigest()
    assert artifact_hash == receipt["artifact_hash"], "artifact changed"
    core = ctypes.CDLL(str((directory / "core.native").resolve()))
    core.temper_initialize.restype = ctypes.c_int
    core.temper_initialize.argtypes = []
    assert core.temper_initialize() == 0
    core.temper_call.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
    core.temper_call.restype = ctypes.c_void_p
    core.temper_free.argtypes = [ctypes.c_void_p]
    core.temper_free.restype = None

    def invoke(request):
        data = request.encode("utf-8")
        size = ctypes.c_size_t()
        result = core.temper_call(data, len(data), ctypes.byref(size))
        assert result, "native call failed"
        try:
            return ctypes.string_at(result, size.value).decode("utf-8")
        finally:
            core.temper_free(result)

    workload = receipt["manifest"]["objective"]["inputs"][0]
    total_rows = check_response(workload, invoke(workload))
    original = json.loads((PACKAGE / "original-example.json").read_text())
    original_request = "\n".join(["2", original["input-0.raw"], original["input-1.raw"]])
    original_response = invoke(original_request)
    assert check_response(original_request, original_response) == 6
    source_rows = [row for name in ["output-0.raw", "output-1.raw"] for row in decode(bytes.fromhex(original[name]))]
    port_rows = [row for file in original_response.split("\n")[1:] for row in decode(bytes.fromhex(file))]
    assert port_rows == source_rows, "original project's example changed semantics"
    for seed in range(64):
        inputs = synthetic(seed, seed * 37 % 301, seed % 8 + 1)
        if seed % 2:
            inputs = [list(reversed(rows)) for rows in inputs]
        request = envelope(inputs, seed % 13 + 1)
        response = invoke(request)
        total_rows += check_response(request, response)
        assert invoke(request) == response, "pure execution/replay differs"

    edges = [(0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF, 0xFFFFFFFF,
              0xFFF8000000000042, ("", "nul:\0", "utf8:東京", "same", "same"))]
    request = envelope([edges, edges, []], 5)
    assert check_response(request, invoke(request)) == 2
    valid = encode(edges)
    malformed = ["", "0\n" + valid.hex(), "1025\n" + valid.hex(), "1", "1\nzz",
                 "1\na", "1\n", "1\n00", "1\n" + (b"\x02" + valid[1:]).hex(),
                 "1\n" + valid[:-1].hex(), "1\n" + (valid + b"\x00").hex(),
                 "1\n" + (b"\x81\x00" + valid[1:]).hex(),
                 "1\n" + encode([(1, 2, 3, 0, ("bad\nentry",))]).hex()]
    # Header with a duplicate dictionary, invalid UTF-8, or out-of-range tag ID.
    for row_data, dictionary, entries in [
        (b"", b"x\nx\n", 2), (b"", b"\xff\n", 1),
        (struct.pack("<IQIQ", 1, 2, 3, 0) + b"\x01\x01", b"x\n", 1),
    ]:
        size = len(row_data) + len(dictionary)
        data = b"".join(map(varint, [1, size, size, int(bool(row_data)), entries, len(row_data)])) + row_data + dictionary
        malformed.append("1\n" + data.hex())
    for request in malformed:
        assert invoke(request) == "error", "invalid input accepted: " + repr(request[:80])

    # The independent oracle must fail on relevant defects, not just pass outputs.
    controls = 0
    original = envelope([edges, edges], 1)
    ordered_inputs = synthetic(5, 8, 2)
    unordered = sorted([row for rows in ordered_inputs for row in rows], key=lambda row: row[:2], reverse=True)
    for control_request, corrupted in [
        (original, "ok\n" + encode(edges).hex()),
        (original, "ok\n" + encode([(1, 2, 3, 0, ())]).hex()),
        (envelope(ordered_inputs, 1), "ok\n" + encode(unordered).hex()),
    ]:
        try:
            check_response(control_request, corrupted)
        except AssertionError:
            controls += 1
    assert controls == 3, "oracle missed a deliberate row-loss/corruption/ordering defect"
    return {"candidate": receipt["candidate"], "artifact_hash": artifact_hash,
            "specification": receipt["specification"], "seeded_cases": 64,
            "seeded_replays": 64, "invalid_cases": len(malformed),
            "checked_rows": total_rows + 2, "negative_controls": controls,
            "original_example_rows": len(source_rows),
            "reviewer": receipt["reviewer"], "test_only": True}


def run(directory, executable):
    """Replay the experiment with the real Temper checker, native ABI and search."""
    directory.mkdir(parents=True, exist_ok=False)
    package = directory / "contract"
    prepare(package)

    def command(name, *arguments, expect_success=True):
        result = subprocess.run([str(executable), *map(str, arguments)], text=True, capture_output=True)
        (directory / (name + ".stdout")).write_text(result.stdout)
        (directory / (name + ".stderr")).write_text(result.stderr)
        if expect_success and result.returncode:
            raise RuntimeError(result.stderr + result.stdout)
        return result

    frozen = [(package / name).read_bytes() for name in ["Spec.lean", "manifest.json"]]
    verified = json.loads(command("verify", "verify", package).stdout)
    assert verified["verified"] is True
    # Candidate files are read only after the specification-proof gate succeeds.
    artifact = directory / "winner"
    candidates = ["Baseline.lean", "RejectRowLoss.lean", "TreeDictionary.lean", "CachedIndexes.lean", "Baseline.lean"]
    arguments = ["optimize", package]
    for candidate in candidates:
        arguments.extend(["--candidate", PACKAGE / candidate])
    arguments.extend(["--budget", len(candidates), "--output", artifact])
    search = json.loads(command("optimize", *arguments).stdout)
    assert search["specification"] == verified["specification"]
    assert frozen == [(package / name).read_bytes() for name in ["Spec.lean", "manifest.json"]], "contract mutated"
    history = search["history"]
    assert history[0]["accepted"] and not history[1]["accepted"] and history[1]["candidate"] is None
    assert any(attempt["accepted"] and attempt["candidate"] != history[0]["candidate"] for attempt in history[2:]), "no measured improvement; inspect optimize.stdout"
    (directory / "validation.json").write_text(json.dumps(test_artifact(artifact), indent=2) + "\n")
    denied = command("unreviewed-invoke", "invoke", artifact, "--tenant", "default", "--input", "1\n010000000000", expect_success=False)
    assert denied.returncode != 0 and "deployment review is required" in denied.stderr
    print(json.dumps({"specification": verified["specification"], "candidate": search["candidate"],
                      "artifact": str(artifact), "deployment_review_required": True,
                      "validation": str(directory / "validation.json")}, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare_parser = commands.add_parser("prepare")
    prepare_parser.add_argument("directory", type=Path)
    test_parser = commands.add_parser("test")
    test_parser.add_argument("artifact", type=Path)
    run_parser = commands.add_parser("run")
    run_parser.add_argument("directory", type=Path)
    run_parser.add_argument("--temper-lean", type=Path, required=True)
    request_parser = commands.add_parser("request")
    request_parser.add_argument("--seed", type=int, default=182)
    request_parser.add_argument("--rows", type=int, default=8)
    request_parser.add_argument("--files", type=int, default=2)
    request_parser.add_argument("--outputs", type=int, default=2)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.directory)
    elif args.command == "test":
        print(json.dumps(test_artifact(args.artifact), indent=2))
    elif args.command == "run":
        run(args.directory.resolve(), args.temper_lean.resolve())
    elif args.command == "request":
        if args.rows < 0 or args.files <= 0 or not 1 <= args.outputs <= 1024:
            parser.error("rows must be nonnegative, files positive, outputs 1..1024")
        print(envelope(synthetic(args.seed, args.rows, args.files), args.outputs), end="")


if __name__ == "__main__":
    main()
