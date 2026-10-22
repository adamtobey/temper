# RAW compaction: Lean-only Temper port

## Binary FFI and sustained search update — 2026-10-22

The current app is [binary/](test-fixtures/lean/raw-compaction/binary/): all
compaction, parsing, encoding and proofs are Lean. Python prepares workloads,
proposes candidates and independently checks results; no Rust app/service was
introduced. The original string-ABI experiment is preserved below as history.
Its elapsed-time numbers are not directly comparable with these larger CPU-time
workloads.

### Rust ↔ Lean boundary

Temper now supports the exact checked type
`UInt64 → Array ByteArray → Except String (Array ByteArray)` through
`"abi": "buffers_v1"`. The numeric argument is the requested output count.
Binary input, multiple binary outputs and typed errors replace hex transport.
Existing string apps remain supported.

The trusted adapter uses [argumentcomputer/lean-ffi at the pinned revision](https://github.com/argumentcomputer/lean-ffi/tree/b4beefd55352ad1d864562b1f15cb52db6186b2d).
It is built on demand with a locked dependency graph against Temper's pinned
Lean 4.30.0-rc2 headers/runtime, not the upstream checkout's default toolchain.
Ordinary Rust builds do not require Lean or libclang; `temper lean setup-ffi`
explicitly prepares dependencies, while verification builds offline.

`NativeCore::with_buffers` copies each input once into Lean, then lends
Lean-owned output slices to a Rust callback without an output copy. RAII releases
objects on the calling thread, then finalizes its Lean context. Views cannot
escape; nested calls fail explicitly instead of corrupting thread ownership or
deadlocking. `invoke_buffers` is the owned-output convenience API. Native C
handles must stay on their creating thread and must not be nested.

Persisted reload testing caught random temporary archive paths in macOS debug-map
metadata from Rust's prebuilt runtime objects. The native link now omits that
debug metadata (`-Wl,-S`), rather than weakening artifact comparison. The binary
save/rebuild/review/reload regression failed before the fix and passes after it.

A separate resident-input identity benchmark measured the complete Rust APIs,
with seven alternating samples, untimed initialization/warmup and process CPU
time:

| Input | Existing string API ns/call | Borrowed buffers ns/call |
| --- | ---: | ---: |
| 64 bytes | 248.6 | 251.1 |
| 1 MiB | 1,503,240 | 13,200 |

The large-call boundary used about 114× less CPU; tiny calls were roughly
unchanged. This is **not** a 114× RAW compaction improvement or a universal claim.
Reproduce with `cargo test -p temper-lean --test ffi_cost -- --ignored --nocapture`.

### Specification and larger benchmark, frozen first

The binary ABI and more efficient reference reader are a **new contract revision**,
not edits during an optimization episode. The old contract is untouched. The new
specification's complete reference proofs passed Temper before its baseline or
mutation candidates were generated. It retains row permutation/set equality,
RAW-format correctness, float-bit preservation, ordering, balanced partitioning,
dictionary uniqueness/resolution and valid-input success. The reference reader
uses bounded traversal and array dictionary lookup to avoid repeated scans over
whole remaining files.

Frozen binary `Spec.lean` SHA-256:
`24b71f14ec8c3fe7fde93d6befe7dce587f31456e9d2cc360bbe7323e665cdc0`.
Frozen prepared manifest SHA-256:
`edf991f193f65f0523947a54ce4995963b6b9fc13c2d68402e0b5674fef7e8bd`.
The original spec still hashes to
`21477442c0162f5b1c82c5e2d601997ef2e03f71e9e73ce95f2a2927ed216a23`.

| Distribution | Rows | Input files | Total RAW bytes | Unique tags |
| --- | ---: | ---: | ---: | ---: |
| Hot-key/tag skew | 65,536 | 8 | 2,529,628 | 803 |
| High cardinality / similar prefixes | 32,768 | 4 | 5,514,007 | 64,869 |
| Wide Unicode values | 16,384 | 2 | 6,458,702 | 19,026 |

Total: 114,688 rows and 14,502,337 bytes; individual files span 315,681–3,230,071
bytes. Data includes overlapping keys, full-width IDs, variable/repeated/empty
tags, embedded NUL/Unicode, exact duplicate rows and special float bit patterns.
Generator seeds are 182–184; no large generated files or binaries are committed.

The metric is process `cpu_ns` per compaction. Each distribution has five samples
of one complete compaction after untimed warmup; order rotates across samples.
The objective minimizes the arithmetic mean of the three per-distribution medians
with a 2% acceptance threshold. Every profile's median is retained.
Generation, disk loading, hashes, process/module initialization, compilation,
proof checking and warmup are excluded. The timed work includes input copying,
Lean parsing/compaction/serialization, borrowed-result inspection and cleanup.
Files and compiled contract/FFI bytes are snapshotted at the proof gate; subsequent
file/cache changes cannot silently change that episode.

### What the optimization process actually did

Temper's implemented loop now defaults to 32 attempts and 900 seconds per proposer
call, configurable separately from the benchmark's time budget. It continues
after provider errors, empty output and proof failures; it does not end at the
first improvement. It retains verified alternatives, explores another parent after
stalls, avoids repeated source/native executables, and supplies recent rejected
source, diagnostics and per-profile measurements for repair. An apparent gain
must beat both the stored score and a fresh incumbent measurement. Every step
checkpoints full history/source and unreviewed winners. A seed with no successfully
measured generated candidate is reported as inconclusive, not successful search.

The actual campaign used the executable feedback-driven `binary/mutate.py`,
not an LLM. It explores four dictionary orders × two lookup representations ×
three encoding strategies. The current parent's settings and history choose the
next untried mutation; Temper, not the worker, proves and measures each result.
All 24 complete proof templates also typechecked independently.

The 32-attempt run measured the baseline plus 23 new configurations, accepted
three successive improvements, and continued after all three. Attempts 25–32
were recorded empty after this finite family's exhaustion—not eight additional
algorithms. There is no claim of an unbounded or autonomous model search.

| Accepted stage | Mean of medians, CPU ns/compaction |
| --- | ---: |
| Binary baseline, already using prior tag-ID cache | 1,541,956,000 |
| Byte-array part assembly | 1,501,491,333 |
| Incremental row/tag byte encoding | 1,433,559,333 |
| Hash-map tag-ID cache + incremental encoding | 1,391,504,000 |

That campaign observed 9.8% less CPU. A subsequent two-candidate confirmation
under the final hardened checker/linker measured baseline 1,595,305,000 and winner
1,427,281,000 ns: 10.5% less CPU, or 8.8% versus the freshly remeasured incumbent
(1,565,667,000 ns). These are local-host observations with timing variability,
not proved performance bounds. The winning source is committed as
[binary/Optimized.lean](test-fixtures/lean/raw-compaction/binary/Optimized.lean).

The campaign preceded final snapshot/checker/linker hardening, so its receipt
identity differs from the final confirmation. The exact selected source was
proof-checked, rebuilt, benchmarked and independently validated again under the
final code; neither the Lean specification nor workload changed. Final candidate:
`6e032c54525939e9118fa03441632d78a86ace3027fb54cf634fd211124d5f1f`.
Full identities, per-profile results and summarized lineage are in
[binary/results.json](test-fixtures/lean/raw-compaction/binary/results.json).

For broader algorithm search, `binary/model_proposer.py` is a runnable adapter
to an explicitly configured inference command: `TEMPER_LEAN_MODEL_COMMAND` is a
JSON argv array accepting the prompt on stdin and returning final Lean on stdout.
It sends the frozen contract, parent, measurements and recent proof failures to
that command. No model/account is selected implicitly, and no model was invoked
for this experiment. Prompting alone does not guarantee novel/provable improvements.
The same real kernel check/measure/select loop owns either proposer.

### Reproduce the new experiment

Use new output directories, the pinned Lean install, libclang and the sandbox
prerequisites in [docs/lean.md](docs/lean.md). The local run used Rust 1.95 because
the checkout's pinned nightly was unavailable:

```sh
cargo +1.95 build -p temper-lean --target-dir /tmp/temper-lean-target
/tmp/temper-lean-target/debug/temper-lean setup-ffi
python3 test-fixtures/lean/raw-compaction/binary/experiment.py prepare /tmp/raw-binary-contract
/tmp/temper-lean-target/debug/temper-lean verify /tmp/raw-binary-contract
/tmp/temper-lean-target/debug/temper-lean optimize /tmp/raw-binary-contract \
  --seed test-fixtures/lean/raw-compaction/binary/Baseline.lean \
  --proposer test-fixtures/lean/raw-compaction/binary/mutate.py \
  --budget 32 --output /tmp/raw-binary-search
python3 test-fixtures/lean/raw-compaction/binary/experiment.py test /tmp/raw-binary-search
```

Use `--candidate .../Baseline.lean --candidate .../Optimized.lean --budget 2`
instead of seed/proposer to reproduce just the final comparison. For model search,
configure the inference command explicitly and replace the proposer path with
`binary/model_proposer.py`; provider errors remain visible in the history.

### Verification and remaining limits

The final native winner passed an independent strict RAW decoder/oracle on all
three large workloads, 64 generated cases and exact replays, six malformed
requests and the original project's six-row example: 123,963 checked rows plus
that original example. Both semantic multiplicity and binary-format output are
checked. The receipt remains `reviewer: null`; ordinary `invoke-buffers` refused
it with “deployment review is required” and created no output directory.

The complete Lean suite passed (16 tests plus one compile-fail lifetime doctest;
the manual timing test was run separately). Four proposer-adapter tests passed.
The real native HTTP/tenant integration passed, as did `platform_e2e_dst` (6 tests)
and `system_entity_dst` (33 tests). Final Temper build, Rust formatting/clippy,
live isolated health 200, valid CSDL XML, unauthenticated Lean HTTP 401 and the
existing IOA L0/L1/L2/L3 cascade all passed. Full workspace testing stopped at
nine Postgres-container tests because `/var/run/docker.sock` is absent.
The static determinism audit reports 21 existing patterns in unchanged server
files; this change adds none in simulation-visible crates.

Local evidence: `/private/tmp/temper-raw-binary-final/` and its `.search/`
checkpoints; final `/private/tmp/temper-raw-binary-verified-final/`;
`/private/tmp/temper-lean-final-{tests,platform}.log`;
`/private/tmp/temper-lean-verify.2ciHAh/`. The isolated server was stopped.
No actual RAW artifact was approved, installed or deployed.

The theorem still treats the executable reader/encoder as the format contract;
there is no general reader/encoder round-trip theorem. The independent byte oracle
does not extend that formal boundary. Sorting/parsing still materialize rows and
lists; the chosen finite mutation family does not prove global optimality. Native
compiler/runtime/FFI correctness is trusted. Linux CI is configured for the adapter
but this experiment's native ABI and timings were exercised on Apple M4 Max/macOS.

## Original string-ABI experiment (historical)

The following records the earlier small-file experiment. Its string-only ABI,
future-work notes, timings and then-current verification limits describe that
revision, not the binary update above.

This ports the RAW metric compactor from `~/code/raw-lean` to Temper's verified
native Lean path (ADR-0182), then optimizes it against a fixed synthetic CPU-bound
workload. All application logic and proofs are Lean. Python is experiment tooling
for generating data, invoking Temper, and independently checking binary results.

The reproduced winner uses **161,300,200 ns/compaction versus 228,222,400 for the
baseline: 29.3% less elapsed time (1.41× throughput)** on the fixed local benchmark.
The specification remained unchanged. The artifact is unreviewed and undeployed.

The app lives in `test-fixtures/lean/raw-compaction/`, not in the Rust kernel.
Its pure Lean entry point uses Temper's existing native `String → String` ABI.
No app-specific Rust service, IOA state machine, database, or deployment is added.

Source inspected: `~/code/raw-lean`, HEAD
`fab60b7fbde0c186b795ce532e7469a0a425c70d`, including the working tree. That
checkout contains user edits and has not been modified. The logical compaction,
RAW layout, and serialization choices are ported; its filesystem demo is not.

### Specification first

`Spec.lean` is generated from the source RAW/compaction specifications and this
request. Input/output row equality is strengthened to permutation (duplicates
are preserved). Values are opaque 64-bit patterns, preserving NaNs and signed
zero exactly. The contract requires sorted, globally range-partitioned outputs,
exactly M balanced chunks, unique local dictionaries, resolving tag IDs, and the
specified RAW bytes. Valid inputs must succeed; an always-error implementation
does not satisfy the contract.

Format choices inherited from the source serializer: unsigned LEB128, little
endian, RAW version 1, no compression, payload-relative dictionary offsets and
payload sizes. Tags are UTF-8, terminated by LF; embedded LF is invalid.
M must be 1–1024. Empty files are allowed. Equal keys can cross adjacent chunks;
range ordering is non-strict rather than strictly disjoint key sets.

The executable contract includes the reader/encoder and a proved reference
model. Candidate implementations remain separate and may use any algorithm that
proves `Contract.correct`. No specification edits are permitted after the proof
gate while developing or optimizing candidates.

The first Temper proof gate passed **before either candidate was generated**:
specification ID `0029c6d4760348a343f574fd27b4fca90bb4f4f9fa2d9873f694102d257cbe09`.
Frozen `Spec.lean` SHA-256:
`21477442c0162f5b1c82c5e2d601997ef2e03f71e9e73ce95f2a2927ed216a23`.
Frozen generated manifest SHA-256:
`1de3e02b0bb9113036d2dcc480b9779f67db506db2724318fe4113209ae2631b`.

### Environment notes

This work is in the primary Temper checkout on `at/lean-with-raw`, remote
`git@github.com:nerdsane/temper.git`, as requested. App logic is a portable Lean
example fixture rather than kernel behavior. No installed Temper Effort contract
or tracking tool was available in this session; task results are recorded here.
The configured `~/Garden` checkout is absent. Neither prevents this local port.
The Lean tooling source at the branch's kernel revision `3b1839d2` is identical
to the tested tooling revision `3a38d2f1` (the intervening history was reworded).

### Files and execution

| File | Purpose |
| --- | --- |
| [Spec.lean](test-fixtures/lean/raw-compaction/Spec.lean) | Frozen format, reader, semantic obligations, and complete reference proofs |
| [Baseline.lean](test-fixtures/lean/raw-compaction/Baseline.lean) | Source-style merge sort and list dictionary |
| [TreeDictionary.lean](test-fixtures/lean/raw-compaction/TreeDictionary.lean) | Balanced-tree dictionary construction with complete membership/uniqueness proofs |
| [CachedIndexes.lean](test-fixtures/lean/raw-compaction/CachedIndexes.lean) | Proved per-file tag-index cache and byte-identical serialization refinement |
| [RejectRowLoss.lean](test-fixtures/lean/raw-compaction/RejectRowLoss.lean) | Intentionally invalid always-error candidate; must fail checking |
| [benchmark.json](test-fixtures/lean/raw-compaction/benchmark.json) | Synthetic workload parameters and optimization objective |
| [experiment.py](test-fixtures/lean/raw-compaction/experiment.py) | Reproduction runner and independent binary oracle |
| [original-example.json](test-fixtures/lean/raw-compaction/original-example.json) | Actual example RAW bytes from the original project |
| [results.json](test-fixtures/lean/raw-compaction/results.json) | Recorded measurements, lineage, identities, and independent validation |

The ABI is `String → String`. The request is a positive output count followed by
one lowercase-hex RAW input file per line, with no trailing newline:

```text
2
<first RAW file in lowercase hexadecimal>
<second RAW file in lowercase hexadecimal>
```

Success is `ok` followed by exactly M lowercase-hex RAW output files, one per
line. Failure is exactly `error`. `1\n010000000000` compacts one empty RAW file.
The Lean app does not read or write files, contact services, or manage deployment.
Both sorted and unsorted inputs are accepted; malformed files fail closed.

### Steps performed

1. Inspected the original Markdown specs, logical models, proofs, serializer,
   filesystem demo, and actual generated example files. Preserved the source
   checkout, including its uncommitted edits.
2. Generated the new high-level contract and strict binary reader. Added
   multiplicity conservation, global ordering, exact chunk sizes/count, unique
   dictionaries, resolving IDs, canonical output bytes, and valid-request
   totality. Used `UInt64` float bits instead of semantic `Float` equality.
3. Completed the reference proofs. `consistent` constructs a program satisfying
   `correct`; `run_correct`, `plans_valid`, `row_set_equality`, and
   `tagIndex_resolves` cover composition, conservation, and dictionary references.
4. Generated a fixed synthetic workload and manifest with `experiment.py prepare`.
   Ran `temper-lean verify` successfully before generating any candidates.
5. Generated and checked the source-style baseline. Ran the actual compiled
   native core through Temper's benchmark subprocess and C ABI bridge.
6. Proposed a tree-set dictionary. Its first proof did not elaborate; the search
   rejected it and retained the baseline. Corrected only the candidate proof.
   The next trial measured 227,097,900 ns for the baseline and 201,490,500 ns for
   the tree dictionary: an 11.3% improvement on this workload.
7. Generated a further candidate with a tree-map tag-ID cache per output file.
   Proved that every cache lookup equals the original `tagIndex`, each row's
   encoded bytes are unchanged, the complete RAW encoding is unchanged, and
   therefore the candidate satisfies the original total-correctness theorem.
8. Ran the final bounded search in order: baseline, invalid always-error control,
   tree dictionary, cached indexes, baseline regression control. Temper verifies
   each proposal before benchmarking; accepted improvements become the next
   parent without deployment review. The contract/manifest hashes stay fixed.
9. Exercised the native artifacts with the original example and independent
   generated/malformed byte tests. Tested the oracle against deliberate defects.
   Ran the existing `temper-lean` regression suite.

This is a bounded, model-authored search through explicit candidates, not a claim
that an autonomous AlphaEvolve model service was installed. It uses Temper's real
proof-check/compile/measure/select loop. `temper lean optimize --proposer` remains
available for an external generator that consumes the current parent and history.

### Benchmark definition

The objective is to minimize median native `latency_ns` by at least 3% relative to
the current parent. The fixed workload is one complete compaction per iteration:
2,048 rows, eight overlapping input files, four outputs, seed 182. Each sample
runs ten compactions; there are nine samples after an untimed warmup invocation.
The reported unit is therefore **ns/compaction**, not ns/row or time for all
samples. Input RAW bytes total 86,016; the hex transport request is 172,041 bytes.

Each row has shared tags, a high-cardinality host tag, a region tag, and a Unicode
tag. Every eleventh generated row repeats the preceding row exactly. Values
include NaNs with payloads, signed zero, and infinities. Input files are individually
sorted with substantially overlapping keys. No data generation, proof checking,
native compilation, module initialization, disk access, HTTP, or process startup
is inside the timing interval. Request conversion, Lean parsing, sorting,
partitioning, dictionary construction, serialization, response hex encoding, and
bridge allocation/free **are** timed.

Workload SHA-256:
`5acb4a64e288dd896281b3bd8d07328d9a666c9cd800048da0e33bffc44a1a2f`.
Host: Apple M4 Max, arm64 macOS. Lean 4.30.0-rc2, release commit
`3dc1a088b6d2d8eafe25a7cd7ec7b58d731bd7cc`; native compilation uses `-O3`.
The benchmark is single-threaded and CPU-bound, but uses monotonic **elapsed**
nanoseconds, not CPU-time accounting or hardware instruction counters. This is a
local-host experiment, not a dedicated-host or universal performance guarantee.

### Recorded results

The fresh end-to-end reproduction produced the following history under the same
specification ID and exact workload:

| Attempt | Median ns/compaction | Selection |
| --- | ---: | --- |
| Source-style baseline | 228,222,400 | Initial parent |
| Always-error negative control | Not benchmarked | Rejected by Lean proof checker |
| Tree-set dictionary | 203,440,500 | Accepted improvement |
| Tree dictionary + cached tag IDs | 161,300,200 | Accepted improvement; winner |
| Baseline proposed again | 229,017,900 | Verified, measured, rejected as slower |

Improvement is `(228222400 - 161300200) / 228222400 = 29.3%`; speedup is
`228222400 / 161300200 = 1.41×`. These are observed benchmark results, not proved
performance bounds. An earlier full search measured 251,855,700 → 163,968,600 ns
(34.9%); baseline variability is why the reproduced 29.3% is the headline.
Both histories are retained in [results.json](test-fixtures/lean/raw-compaction/results.json).

Winner candidate ID:
`55559266cd739968326a8983f574a12c57edad9d6365a285adc4d8a0e1acb19f`.
Native file SHA-256:
`d76469a50a055265d3d06bde7ee62d8152e795f82faf49c5db3287154a72dc39`.
Temper's length-framed artifact hash:
`8703d68e5e5a8e915bf55488897ff34a7f7f9448042abcc5d9d7f11eb9d613b2`.
The same exact native artifact won both full searches; neither receipt has a
reviewer. The retained reproduction is at
`/private/tmp/temper-raw-lean-reproduction/`, including the review-gate refusal.

### Reproduce

Install the pinned Lean toolchain and the platform sandbox prerequisites described
in [docs/lean.md](docs/lean.md). Python 3.9+ uses only its standard library. The
local Rust build used the installed 1.95 toolchain because this checkout's pinned
nightly was unavailable. Use a **new** output directory for every run:

```sh
cargo +1.95 build -p temper-lean --offline --target-dir /tmp/temper-lean-target
python3 test-fixtures/lean/raw-compaction/experiment.py run \
  /tmp/raw-compaction-reproduction \
  --temper-lean /tmp/temper-lean-target/debug/temper-lean
```

The runner materializes `contract/manifest.json` and the unchanged `Spec.lean`,
checks the specification first, evaluates all five proposals, writes `winner/`
with `history.json`, `receipt.json`, and `core.native`, runs independent native
validation, and confirms that the ordinary invoke command refuses deployment
without review. CLI stdout/stderr and `validation.json` are retained beside them.
It deliberately does **not** approve or deploy an artifact. Without `--offline`,
Cargo may download dependencies on a machine without a populated cache.

Individual steps, using the same prepared package and frozen objective:

```sh
python3 test-fixtures/lean/raw-compaction/experiment.py prepare /tmp/raw-contract
/tmp/temper-lean-target/debug/temper-lean verify /tmp/raw-contract
/tmp/temper-lean-target/debug/temper-lean optimize /tmp/raw-contract \
  --candidate test-fixtures/lean/raw-compaction/Baseline.lean \
  --candidate test-fixtures/lean/raw-compaction/RejectRowLoss.lean \
  --candidate test-fixtures/lean/raw-compaction/TreeDictionary.lean \
  --candidate test-fixtures/lean/raw-compaction/CachedIndexes.lean \
  --candidate test-fixtures/lean/raw-compaction/Baseline.lean \
  --budget 5 --output /tmp/raw-winner
python3 test-fixtures/lean/raw-compaction/experiment.py test /tmp/raw-winner
```

The package is generated rather than committing a 172 KB hex benchmark string.
Repeated preparation produced byte-identical manifests in this environment.
Changing the workload, objective, or specification creates a **new** contract; it
is not permitted as an implementation optimization. Re-run the proof gate first.

### Proof boundary and limitations

The correctness theorem is over all requests accepted by the fixed Lean reader,
not only the synthetic benchmark. Invalid inputs must return `error`. Successful
responses must be the defined RAW byte encoding of output rows with the proved
semantic properties. This is not merely a theorem about an unused model: the
cached candidate proves its actual native entry function equals the proved
function, down to serialized bytes. Temper checks the exact obligation on
`Candidate.program` and audits helper declarations too.

The encoder itself is the executable byte-format specification; there is no
separate general theorem that `readRaw (rawBytes out) = some out.rows`. The
independent Python byte oracle exercises that compatibility for the original
example, benchmark, edge cases, and generated inputs. Those tests are not a
replacement for or expansion of the formal theorem. The reader's accepted-input
language is likewise part of the frozen specification, rather than a separately
proved equivalence to every possible external RAW implementation.

Compiler/runtime/native ABI correctness is trusted, as chosen in ADR-0182. This
port supports uncompressed version 1, not compression or other RAW versions. It
has no streaming/file-backed interface, compressed-size optimization, strict
nonoverlapping equal-key boundaries, or formal time/memory/termination-cost bound.
Proof checking establishes functional correctness, not performance or freedom
from resource exhaustion on huge inputs. The fixed string ABI adds hex transport
and allocation costs; a future byte/stream ABI would be a separate kernel change.

The reference parser still uses lists and repeated length/traversal operations,
and sorting still gathers the rows before merge sort. The measured improvements
do not imply these are optimal. A streaming reader, efficient byte buffers,
proved sorted-input multiway merge, multiple workload distributions, and CPU-time
or instruction-count metrics are useful future experiments, each under a newly
reviewed/frozen specification or proved refinement as appropriate.

The generated cases are pure-function property/integration tests with exact
replay, **not distributed deterministic simulation**. No actor, storage, HTTP,
authorization, or service behavior was changed. The relevant live path here is
the standalone native Lean CLI and its compiled ABI; no Rust app/service layer
or IOA state machine was introduced.

### Verification performed

- Temper's actual Lean proof audit accepted the frozen specification and all
  three valid candidates, including every helper proof. The deliberate
  always-error candidate failed the exact `Contract.correct Candidate.program`
  obligation. No `sorry`, custom axiom, native proof shortcut, or spec change was
  used to admit an implementation.
- Independent native checks on the final candidate covered the 2,048-row
  benchmark, 64 additional generated cases and their exact replays, maximum-width
  integer fields, empty inputs, more outputs than rows, duplicate tags, embedded
  NUL, Unicode, special float bits, and 16 malformed requests. They inspected
  11,325 rows plus the original project's six-row example. Three deliberately
  corrupted responses demonstrated that the oracle catches row loss, changed
  rows, and unsorted output. These are not exhaustive input-space tests.
- `cargo +1.95 test -p temper-lean --offline --target-dir
  /private/tmp/temper-lean-target`: all 10 existing library/integration tests
  passed, including native ABI, proof admission, artifact review/tampering,
  custom metrics, and optimization selection. Full workspace and HTTP/DST suites
  were not rerun for this fixture/documentation-only change.
- Repeated package generation produced the same manifest; source and generated
  `Spec.lean` still have their pre-generation hash. `git diff --check` passed.

The native ABI test harness loads only this locally produced artifact for
**testing**, checks its content hash, and does not authorize deployment or write a
reviewer into its receipt. Deployment still goes through Temper's review-gated
loader. Nothing was approved, served, or deployed as an app in this experiment.
