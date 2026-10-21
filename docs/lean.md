# Lean app cores

ADR-0182 adds a verified Lean core behind Temper's Rust service layer. The process
is the same as IOA: generate a specification from conversation, verify it, freeze
it, and generate implementations against it. Each implementation must prove every
fixed obligation. Search may immediately use a verified improvement as its next
parent; deployment requires explicit review of that exact artifact.

## Package and proof boundary

A component directory contains `manifest.json` and `Spec.lean`. See
[the complete interoperability fixture](../test-fixtures/lean/echo/manifest.json).
The manifest binds a tenant, component, exact Lean release, specification-proof
names, implementation-obligation names, and an optional benchmark objective.
Unknown manifest fields, empty proof lists, and invalid budgets are errors.

`Spec.lean` declares predicates under `Contract`. Names in `specification_proofs`
must already be proved propositions. Each name in `obligations` is a predicate
over the implementation. A candidate imports `Spec`, defines
`Candidate.program : String → String`, and proves `Candidate.<obligation>` with
type `Contract.<obligation> Candidate.program` for every obligation. The host
constructs and checks those expressions directly; candidates cannot choose a
replacement theorem statement or omit a required proof.

The first ABI uses UTF-8 strings, including embedded NULs. Apps can encode typed
state/action/result envelopes as JSON. Rust owns authentication, persistence,
OData/CSDL, external API calls, and effects. The service endpoint itself is a pure
core invocation, not an entity transition or a persistence transaction. A Rust
app commits resulting state/effects through its existing Temper integration.
IOA portions retain their existing cascade. Overlapping IOA/Lean contracts need
their own correspondence proofs; no automatic IOA-to-Lean translation is implied.

## Tooling

Install the pinned toolchain with `elan toolchain install
leanprover/lean4:v4.30.0-rc2`. Linux also needs Bubblewrap (`bwrap`) with working
unprivileged user namespaces. macOS uses `sandbox-exec` and an Xcode/Command Line
Tools SDK. Unsupported/missing isolation fails closed.
This initial native ABI accepts that exact Lean release; upgrades require an
adapter/checker update and fresh artifact verification.

The commands below are available as `temper lean ...`, or independently through
`cargo run -p temper-lean -- ...`. The independent binary avoids building the
entire server when developing a core.

```sh
# First complete the specification-proof gate, before generating a candidate.
temper lean verify test-fixtures/lean/echo

# Check a generated implementation and retain its unreviewed native artifact.
temper lean check test-fixtures/lean/echo \
  test-fixtures/lean/echo/Optimized.lean --output /tmp/lean-checked

# Demonstrate optimization under the SAME specification and benchmark.
temper lean optimize test-fixtures/lean/echo \
  --candidate test-fixtures/lean/echo/Baseline.lean \
  --candidate test-fixtures/lean/echo/Optimized.lean \
  --budget 2 --output /tmp/lean-search

# Review the exact candidate ID printed by check/optimize, before deployment.
temper lean review /tmp/lean-search --expected-id CANDIDATE_ID --reviewer USER
temper lean invoke /tmp/lean-search --tenant default --input 'hello'

# Load a reviewed core into the ordinary Temper Rust server.
temper serve --lean-artifact /tmp/lean-search
```

Output directories must be new. `history.json` retains parent identities,
benchmark measurements, rejected proofs, and diagnostics, including when all
candidates fail. `receipt.json` retains the immutable contract, implementation,
proof sources, and exact native-artifact hash. Loading a stored artifact rechecks
the proofs and rebuilds the native library, then compares both identities with
the stored bytes. These checks deliberately favor correctness over startup speed.
A different toolchain/checker/native build invalidates an old receipt.

Review is a local operator action. The service loads operator-controlled artifact
directories; this is not a public artifact-upload or reviewer-authentication API.
Rust callers must authenticate reviewers before invoking the administrative
review API. Review of a parent never authorizes deployment of a descendant.

## Rust service and authorization

`temper_lean::native::ReviewedCandidate::load` returns a `NativeCore`. Its
`invoke(tenant, input)` method requires the explicit owning tenant. Native calls
are serialized, and the wrapper handles Lean initialization, object ownership,
and foreign-thread registration. The compiler, native runtime, and wrapper are
trusted; native execution shares the Rust service's process failure boundary.

The server provides `POST /api/lean/{component}/invoke` with JSON
`{"input":"..."}`. The response contains `output` and the exact `candidate` ID.
Existing bearer authentication and tenant middleware apply. The request needs
the ordinary Cedar permission `invoke_lean` on `LeanComponent::"{component}"` in
its tenant, for example:

```cedar
permit(
  principal == Agent::"my-agent",
  action == Action::"invoke_lean",
  resource == LeanComponent::"echo"
);
```

The existing policy plane manages this permit. Startup does not grant it
implicitly. Missing credentials return 401, Cedar denials use the existing
session-based pending-decision flow and return 403, and an authorized request for a component
not installed in that tenant returns 404.

## Search and objective specifications

An optimization objective specifies a metric, minimize/maximize direction, input
workloads, sample/iteration counts, and minimum fractional improvement. Built-in
metrics are `latency_ns`, `throughput_per_second`, and `artifact_bytes`. Every
candidate exercises the actual native library against the workload in a bounded
subprocess. Latency samples are aggregated by the median; throughput derives from
the same complete workload. Results describe this benchmark on this host, not a
universal performance guarantee. A failed/inconclusive generation consumes budget
and leaves the last accepted parent intact.

Other metrics, including memory or execution cost, use `metric: "custom"` and an
`external` object containing `program` (absolute executable path), `sha256`
(ordinary SHA-256), and `metric` (the metric's name). The host snapshots and hashes
that benchmark executable before generation. It receives the fixed binary
workload filename and verified native-library filename as two positional
arguments, and must print one positive finite number. It runs in the same bounded
sandbox. The workload file contains little-endian u64 sample count, iteration
count, input count, followed by length-prefixed UTF-8 inputs. The executable must
actually measure the specified objective; reviewing benchmark adequacy is part of
specification review, independent of Lean proof validity.

For iterative generation, pass `--proposer /absolute/path/to/worker` instead of
`--candidate`. The external app worker receives a JSON context filename containing
the fixed specification/manifest, current accepted parent's source, and history.
It emits a candidate's Lean source on stdout, or empty output to finish. Each
invocation has a 120-second budget. Failures are recorded and bounded by the
candidate budget. TemperPaw owns model selection, prompts, and app-specific
generation; the kernel implements the evaluation/selection protocol. The Rust
`Proposer` trait exposes the same loop to an existing orchestrator.

## Supported initial scope

The native core path accepts pure Lean definitions and proofs. It excludes user
initializers, unsafe/partial definitions, custom elaborators, executable commands,
and candidate-supplied runtime attributes. This conservative source boundary keeps
execution hooks outside a claim proved only about pure functions. The kernel also
replays declarations, checks exact obligation types and fixed definitions, audits
every component declaration (including unlisted helpers), and checks native
substitution attributes and transitive axioms. `sorry`/`admit`,
custom proof axioms, and compiler-backed proof shortcuts are rejected. Standard
Lean logical axioms remain permitted; standard runtime primitives are trusted.
Only the pinned toolchain's libraries and supplied specification are available;
arbitrary Lake dependencies/build scripts and Mathlib packages are not installed.
Custom notation and interpolated syntax are outside the initial source fragment.

The echo fixture is intentionally a complete, small kernel interoperability
example. It demonstrates the proof-first workflow, native Rust calls,
benchmark-defined improvement, artifact review, and tenant/Cedar routing. It is
not an application-specific workflow in the kernel. Broader app contracts, schema
migrations, CSDL generation, and additional app-testing methods remain separate
extensions. Existing platform tests and DST remain required.

For a nontrivial Lean-only app using this existing path, see the
[RAW compaction port and experiment](../RAW_LEAN_PORT.md). Its complete format and
row-preservation contract, fixed synthetic benchmark, proved optimization
candidates, and independent byte-level tests live under
`test-fixtures/lean/raw-compaction/`. Run its `experiment.py prepare` command to
materialize a package with a reproducible benchmark manifest before verification.

## Verification

```sh
cargo test -p temper-lean
cargo test -p temper-platform --test lean_native
```

The tests include deliberate missing/wrong/incomplete proofs, unsupported axioms,
artifact substitution, wrong-tenant reviews, optimization/reload, 256 seeded
selection workloads, and 32 twice-replayed tenant/authorization workloads through
the real platform router and native core. They require the real Lean installation
and sandbox; no proof checker is mocked.
