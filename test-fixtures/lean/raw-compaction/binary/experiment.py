#!/usr/bin/env python3
"""Binary RAW benchmark preparation and independent native validation.

File generation and checksums happen here, before the Temper proof/search gate.
The native benchmark preloads all files before starting its CPU clock.
"""
import argparse
import collections
import ctypes
import hashlib
import json
from pathlib import Path
import random
import shutil
import struct
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from experiment import encode, decode, synthetic

PACKAGE = Path(__file__).resolve().parent


def profile_rows(profile):
    rng = random.Random(profile["seed"])
    files = [[] for _ in range(profile["files"])]
    previous = None
    for i in range(profile["rows"]):
        org = rng.randrange(4) if rng.randrange(10) else rng.getrandbits(32)
        metric = rng.randrange(128) if rng.randrange(4) else rng.getrandbits(64)
        bits = rng.choice([rng.getrandbits(64), 0x8000000000000000,
                           0x7FF8000000001234, 0x7FF0000000000000])
        tags = ["env:prod", "service:payments", "region:%02d" % rng.randrange(32)]
        if profile["name"] == "hot-skew":
            tags += ["host:%04d" % rng.randrange(512)]
            tags += ["attribute:%03d" % rng.randrange(256) for _ in range(rng.randrange(9))]
        elif profile["name"] == "high-cardinality":
            tags += ["request:%08d:" % i + "similar-prefix-" * 5,
                     "tenant:%05d" % rng.randrange(20000)]
            tags += ["attribute:%05d" % rng.randrange(20000) for _ in range(rng.randrange(6))]
        else:
            tags += ["unicode:東京☃\0" + "wide-value-" * 24 + str(i)]
            tags += ["dimension:%04d:" % rng.randrange(4096) + "東京:payments:service:" * 3
                     for _ in range(8 + rng.randrange(17))]
        if i % 7 == 0:
            tags += [tags[-1], ""]
        row = (org, metric, (1_700_000_000 + i) & 0xFFFFFFFF, bits, tuple(tags))
        if i % 11 == 0 and previous is not None:
            row = previous
        previous = row
        files[i % len(files)].append(row)
    return [sorted(rows, key=lambda row: row[:2]) for rows in files]


def prepare(directory):
    directory.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(PACKAGE / "Spec.lean", directory / "Spec.lean")
    config = json.loads((PACKAGE / "benchmark.json").read_text())
    workloads, metadata = [], []
    for profile in config["profiles"]:
        inputs = profile_rows(profile)
        files, sizes = [], []
        for index, rows in enumerate(inputs):
            data = encode(rows)
            path = directory / (profile["name"] + "-%02d.raw" % index)
            path.write_bytes(data)
            files.append({"path": str(path.resolve()), "sha256": hashlib.sha256(data).hexdigest()})
            sizes.append(len(data))
        workloads.append({"name": profile["name"], "parameter": profile["outputs"], "files": files})
        all_rows = [row for rows in inputs for row in rows]
        metadata.append({**profile, "bytes": sum(sizes), "file_bytes": sizes,
                         "duplicate_rows": len(all_rows) - len(set(all_rows)),
                         "unique_tags": len({tag for row in all_rows for tag in row[4]})})
    manifest = {
        "version": 1, "tenant": "default", "component": "raw-compaction-binary",
        "toolchain": "leanprover/lean4:v4.30.0-rc2", "abi": "buffers_v1",
        "specification_proofs": ["consistent", "run_correct", "row_set_equality", "tagIndex_resolves"],
        "obligations": ["correct"],
        "objective": {"metric": config["metric"], "direction": "minimize", "inputs": [],
                      "workloads": workloads, "samples": config["samples"],
                      "iterations": config["iterations"], "seconds": config["seconds"],
                      "minimum_improvement": config["minimum_improvement"]},
    }
    (directory / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    record = {"profiles": metadata, "total_bytes": sum(m["bytes"] for m in metadata),
              "spec_sha256": hashlib.sha256((directory / "Spec.lean").read_bytes()).hexdigest(),
              "manifest_sha256": hashlib.sha256((directory / "manifest.json").read_bytes()).hexdigest(),
              "python": sys.version.split()[0], "generation_timed": False, "file_loading_timed": False}
    (directory / "workload.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record, indent=2))


class Input(ctypes.Structure):
    _fields_ = [("data", ctypes.c_void_p), ("length", ctypes.c_size_t)]


class Native:
    """Only local test execution; never a deployment review."""
    def __init__(self, artifact):
        self.receipt = json.loads((artifact / "receipt.json").read_text())
        data = (artifact / "core.native").read_bytes()
        assert hashlib.sha256(struct.pack("<Q", len(data)) + data).hexdigest() == self.receipt["artifact_hash"]
        self.core = ctypes.CDLL(str((artifact / "core.native").resolve()))
        self.core.temper_initialize.restype = ctypes.c_int
        assert self.core.temper_initialize() == 0
        self.core.temper_buffers_call.argtypes = [ctypes.c_uint64, ctypes.POINTER(Input), ctypes.c_size_t]
        self.core.temper_buffers_call.restype = ctypes.c_void_p
        self.core.temper_buffers_status.argtypes = [ctypes.c_void_p]
        self.core.temper_buffers_status.restype = ctypes.c_int
        self.core.temper_buffers_count.argtypes = [ctypes.c_void_p]
        self.core.temper_buffers_count.restype = ctypes.c_size_t
        self.core.temper_buffers_view.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
        self.core.temper_buffers_view.restype = ctypes.c_void_p
        self.core.temper_buffers_free.argtypes = [ctypes.c_void_p]

    def invoke(self, count, files):
        storage = [ctypes.create_string_buffer(data) for data in files]
        inputs = (Input * len(files))(*(Input(ctypes.cast(data, ctypes.c_void_p), len(raw)) for data, raw in zip(storage, files)))
        result = self.core.temper_buffers_call(count, inputs, len(files))
        assert result
        try:
            if self.core.temper_buffers_status(result):
                size = ctypes.c_size_t()
                data = self.core.temper_buffers_view(result, 0, ctypes.byref(size))
                raise ValueError(ctypes.string_at(data, size.value).decode())
            outputs = []
            for index in range(self.core.temper_buffers_count(result)):
                size = ctypes.c_size_t()
                data = self.core.temper_buffers_view(result, index, ctypes.byref(size))
                outputs.append(ctypes.string_at(data, size.value))
            return outputs
        finally:
            self.core.temper_buffers_free(result)


def check(files, count, outputs):
    assert len(outputs) == count
    inputs = [row for file in files for row in decode(file)]
    chunks = [decode(file) for file in outputs]
    rows = [row for chunk in chunks for row in chunk]
    assert collections.Counter(inputs) == collections.Counter(rows), "row/float/tag multiplicity"
    assert [row[:2] for row in rows] == sorted(row[:2] for row in rows), "global ordering"
    remaining = len(rows)
    for i, chunk in enumerate(chunks):
        size = (remaining + count - i - 1) // (count - i)
        assert len(chunk) == size, "balance"
        remaining -= size
    return len(rows)


def test(artifact):
    native = Native(artifact)
    original = json.loads((PACKAGE.parent / "original-example.json").read_text())
    original_inputs = [bytes.fromhex(original[name]) for name in ["input-0.raw", "input-1.raw"]]
    original_outputs = native.invoke(2, original_inputs)
    assert check(original_inputs, 2, original_outputs) == 6
    expected = [row for name in ["output-0.raw", "output-1.raw"] for row in decode(bytes.fromhex(original[name]))]
    assert [row for data in original_outputs for row in decode(data)] == expected
    total = 0
    for workload in native.receipt["manifest"]["objective"]["workloads"]:
        files = [Path(file["path"]).read_bytes() for file in workload["files"]]
        total += check(files, workload["parameter"], native.invoke(workload["parameter"], files))
    for seed in range(64):
        files = [encode(rows) for rows in synthetic(seed, seed * 37 % 301, seed % 8 + 1)]
        count = seed % 13 + 1
        outputs = native.invoke(count, files)
        total += check(files, count, outputs)
        assert native.invoke(count, files) == outputs
    for count, files in [(0, [encode([])]), (1025, [encode([])]), (1, []), (1, [b"bad"]),
                         (1, [encode([]) + b"\0"]), (1, [b"\x81\0\0\0\0\0\0"])]:
        try:
            native.invoke(count, files)
        except ValueError:
            pass
        else:
            raise AssertionError("invalid RAW request accepted")
    result = {"candidate": native.receipt["candidate"], "artifact_hash": native.receipt["artifact_hash"],
              "seeded_cases": 64, "replays": 64, "checked_rows": total, "invalid_requests": 6,
              "original_example_rows": 6, "test_only": True, "reviewer": native.receipt["reviewer"]}
    print(json.dumps(result, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["prepare", "test"])
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.directory.resolve())
    else:
        test(args.directory.resolve())


if __name__ == "__main__":
    main()
