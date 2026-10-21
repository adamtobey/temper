# ADR-0182: Lean verified apps and proof-preserving optimization

- Status: Accepted
- Date: 2026-10-21
- Deciders: Temper core maintainers
- Related:
  - [ADR-0016: Verification Cascade Hardening](0016-verification-cascade-hardening.md)
  - [ADR-0017: Platform Deterministic Simulation Testing](0017-platform-deterministic-simulation-testing.md)
  - [ADR-0030: Hash-Gated Verification Cascade](0030-hash-gated-verification.md)
  - [ADR-0120: Directed Evolution Control Plane](0120-directed-evolution-control-plane.md)
  - [ADR-0181: Strict typed spec reading](0181-strict-typed-spec-reading.md)
  - `.vision/AGENT_OS.md`
  - `crates/temper-spec`, `crates/temper-jit`, `crates/temper-verify`
  - `crates/temper-server/src/registry`, `crates/temper-wasm`, `crates/temper-platform`

## Context

Temper currently defines application behavior with IOA TOML state machine
specifications and CSDL data contracts. The verifier checks the IOA specifications,
and the runtime executes transition tables derived from them. Computation and
external integrations extend that model, but are not automatically covered by the
state machine's proofs.

We want an additional path in which an app's specification is expressed in Lean:
types, definitions, theorem statements, and proofs. An implementation is obtained
by finding executable Lean code and proofs that it satisfies the specification.
Lean may replace the IOA behavioral specification or supplement it. An
AlphaEvolve-style search then improves that implementation while preserving all
required theorems.

The important separation is between choosing the desired behavior and finding an
implementation of it. Implementation search must not repair a failed proof by
changing what the app promises. The optimization process has the same restriction.

Lean distinguishes kernel checking from compilation. Its kernel checks terms in
the logical theory; the compiler produces executable code. These are separate
trust boundaries. A proof about a Lean definition does not by itself establish
that a Temper deployment executes that definition faithfully. See the
[Lean elaboration and compilation reference](https://lean-lang.org/doc/reference/latest/Elaboration-and-Compilation/).

## Decision

This proposal records the clarified direction below. Hybrid Lean/Rust apps,
component-level proof completion before implementation generation, freedom to
choose any theorem-satisfying implementation, benchmark-defined optimization,
native Lean integration, and review before deployment are decisions. Follow
Temper's existing IOA process: specify, verify, then derive the implementation.
Detailed interfaces and the remaining questions are still proposals.

### 1. Add Lean as a first-class specification path

Support three explicit modes:

| Mode | Behavioral contract | Verification of that contract |
| --- | --- | --- |
| IOA | Existing IOA specification | Existing verification cascade |
| Lean | Lean definitions and required propositions | Lean proof checker |
| Hybrid (initial target) | Lean core plus Rust APIs and services, with IOA where useful | Lean proof checker for the core; existing verification for IOA portions; explicit boundary contracts |

The Lean path must be usable without authoring an IOA state machine. An internal
runtime adapter must not quietly turn an IOA file into a second, independently
maintained source of behavior for a Lean-only app.

Prioritize hybrid apps: specify and optimize the core functionality in Lean, and
implement external APIs and the service layer in Rust using Temper's existing
capabilities. IOA may still define lifecycle orchestration where useful; a hybrid
app is not required to duplicate its Lean core as an IOA specification.

Every boundary must identify the behavior owned by Lean, Rust, and any IOA
specification. Where Lean and IOA describe overlapping behavior, establish the
required refinement or equivalence with checked correspondence proofs. Where Rust
implements an external service boundary, state its interface and assumptions
explicitly. Complete proofs of the Lean core do not imply a proof of the Rust
service layer or external systems.

CSDL remains the proposed external data and action contract. Whether CSDL is
generated from Lean declarations or supplied alongside them remains open.

### 2. Separate the fixed contract from the candidate and its proofs

A Lean specification contains the vocabulary that gives its requirements meaning:
public input/output types, allowed behaviors, preconditions, environmental
assumptions, and required propositions. It can also contain proved supporting
lemmas. Requirements about a not-yet-selected implementation are predicates over
an implementation, rather than axioms asserting that an unknown program works.

Development can start with a small component and add components over time, but
proofs for each component must be complete before any implementation of that
component is generated. An incomplete component is not eligible for generation.
The declared scope must include its required behavior and composition assumptions;
incremental scope is not permission to leave required proofs unfinished.

Follow the same overall process as IOA Temper, using Lean for the specification
and its verification:

1. Generate the component's abstract specification from conversation.
2. Complete and check all required proofs about that specification in Lean.
3. Freeze the verified specification and its semantic dependencies.
4. Generate an implementation against that fixed specification, then check proofs
   that the concrete implementation satisfies every required theorem.
5. Optimize implementations against the same specification and declared benchmarks.
6. Review the selected verified artifact before deployment through Rust services.

Specification proofs precede implementation generation. Conformance proofs relate
each subsequently generated implementation to the already verified specification.
These are separate checks in one spec-first workflow. A failed candidate sends
implementation search back to generation without changing the specification.

The accepted result contains a computable implementation and proof terms
establishing every required proposition for that exact implementation:

```text
Specification:
  ImplementationInterface
  Satisfies : ImplementationInterface -> Prop
  required obligations defining Satisfies

Candidate:
  program : ImplementationInterface
  correctness : Satisfies program
```

This is an interface sketch, not a checked Lean example. A future Lean SDK should
make the obligation binding explicit and mechanically checkable. The trusted
checking entry point instantiates the fixed requirements with the candidate's
exported program; the candidate cannot substitute a different `Satisfies`.

“Derive the implementation” means synthesis or search for such a program/proof
pair. It does not assume Lean automatically synthesizes an efficient executable
from arbitrary theorems. Search may fail to find a candidate, and an inconsistent
or unrealizable contract requires a separate specification revision. A theorem
that something exists is insufficient unless the result supplies executable code.

Use a deterministic Lean core with explicit inputs, state, outputs, and requested
effects. Rust services perform persistence and external I/O. For the selected
component, complete the required proofs for initialization, successful behavior,
failures, invariant preservation, and progress before generation. Environment
assumptions and admissible inputs must be reviewed for vacuous requirements.
Adding another component starts the same proof-first process for the new scope.

### 3. Freeze the specification during implementation and optimization

An implementation or optimization episode references one immutable contract
revision. Its identity covers the complete semantic dependency set, not just the
text of named theorems:

- Requirement statements, their supporting definitions, types, preconditions,
  assumptions, and public interface.
- Required-obligation manifest and the trusted entry point that checks every
  obligation against the actual exported implementation.
- Imported specification dependencies, resolved package versions, Lean toolchain,
  and accepted axiom policy.

Candidates may modify executable implementation code and candidate-specific
proofs or lemmas. They may not weaken requirements, introduce stronger
preconditions, change a predicate behind a theorem, omit an obligation, shadow a
required declaration, or replace a dependency to change its meaning.

The verifier supplies the fixed contract and checking entry point from its own
trusted inputs, and checks their identities before accepting a candidate. A
read-only source directory alone is insufficient if the candidate controls import
resolution or the build configuration. Candidate-local helper definitions are
allowed only where the fixed contract deliberately leaves implementation freedom.

A desired behavior change creates a new contract revision and a new search
episode through the ordinary specification workflow. Failure to implement the
current contract does not authorize changing it. This restriction applies to the
first implementation as well as later optimization.

### 4. The Lean verification cascade is proof checking

For Lean requirements, acceptance means that the Lean kernel accepts proofs of
all mandatory obligations for the submitted implementation under the pinned trust
policy. There are no additional SMT, model-checking, simulation, or property-test
levels standing in for missing Lean proofs.

Invoking a normal Lean build and accepting its exit status is insufficient as the
admission protocol. Lean supports axioms and records their dependencies; an
unrestricted axiom could assume the desired result. See the
[Lean axiom reference](https://lean-lang.org/doc/reference/latest/Axioms/).
The proposed protocol therefore:

1. Checks completion of the component's pre-generation proofs, then resolves the
   immutable contract and complete implementation-obligation manifest.
2. Rebuilds in a sandbox with pinned dependencies. A trusted verifier outside
   that sandbox validates the exported declarations, matches the fixed contract,
   and rechecks proofs through the Lean kernel using the supported route for
   that version. Candidate-supplied compiled modules are not trusted evidence.
3. Checks every obligation against the exact candidate program and audits its
   transitive axiom dependencies. Rejects `sorry`/`admit` dependencies, new
   application axioms, and proof shortcuts outside the accepted trust policy.
4. Records the result against the contract, program, proofs, dependencies, checker
   version, and trust policy. A timeout or resource exhaustion is inconclusive
   and cannot produce an accepted result.

**Proposed logical trust policy:** permit the standard logical axioms `propext`,
`Classical.choice`, and `Quot.sound`; reject arbitrary application-specific
axioms and incomplete proofs. Compiler/runtime trust for execution is accepted.
Whether to admit compiler-backed proof methods such as `native_decide`, and the
exact dependency checks for the pinned release, remain policy details to settle.
The admission design should distinguish checking a proof from independently
validating its intended statement. See
[Validating a Lean Proof](https://lean-lang.org/doc/reference/latest/ValidatingProofs/).

This remains one logical verification gate. Contract integrity and obligation
enumeration ensure the checker is checking the intended claim; they are not a
replacement verification cascade. Existing kernel integration tests and DST
continue to verify the platform that hosts Lean apps.

### 5. Bind the proof to what Temper executes

The deployed entry point must be the implementation whose obligations were
checked. Retain identities for its source and proof package, build inputs,
compiler, runtime adapter, and executable artifact. A verification result for one
program must never admit another binary or another exported entry point. Extend
the content-addressed approach of ADR-0030 to these inputs; a source-only cache key
cannot identify a Lean verification result.

Compiled Lean uses a runtime, including low-level primitive implementations. That
runtime is part of the execution trust boundary, as described in the
[Lean runtime reference](https://lean-lang.org/doc/reference/latest/Run-Time-Code/).
Trust the Lean compiler and runtime, including the downstream compilation
toolchain. Record these dependencies and the Rust adapter in the execution trust
base; a verified compiler is not a prerequisite for this path.
Candidate-controlled foreign code, `implemented_by`/`extern` substitutions, or
alternate runtime implementations must not bypass the checked definition. Such
substitutions require a correspondence proof or inclusion in the pinned, reviewed
runtime trust base. General compiler correctness is not established by checking
the application's Lean proofs.

Prefer native compiled Lean called from Rust. Lean exposes a C ABI and exported
symbols, which provide a route for a Rust adapter; this is a design direction,
not a claim that a Temper adapter has been implemented. Lean documents the FFI
as unstable, so pin the toolchain and adapter together. The adapter must handle
runtime/module initialization, thread initialization where required, and object
ownership correctly. See the
[Lean FFI reference](https://lean-lang.org/doc/reference/latest/Run-Time-Code/Foreign-Function-Interface/).

Start with a native library and a narrow Rust wrapper. Assess static versus
dynamic linking, supported platforms, concurrency, upgrades, and failure handling
in an integration spike. If direct embedding proves unsuitable, assess an
isolated native worker before changing the execution target. In-process native
execution shares the Rust service's failure boundary.

**Proposed interface:** the Lean core returns state updates and declared
effects through a versioned interface. Temper retains tenant routing,
authorization, journaling, effect execution, replay, and telemetry. Specify the
semantics of errors, serialization, arithmetic, and effect ordering at that
boundary. Proofs establish only the properties modeled in the contract; external
services and host behavior require explicit assumptions or additional proofs.

Implementation upgrades must also preserve the meaning of existing state and
history. Freeze the persisted representation for an initial prototype, or require
a checked abstraction/migration relation when it changes. Proving each version
correct from its own initial state does not prove a live upgrade is safe. Record
the implementation version needed for replay and rollback.

### 6. Add an AlphaEvolve-style optimization harness

Use iterative generation, evaluation, and selection of candidate implementations.
This follows AlphaEvolve's broad evolutionary search pattern; the proposed
Temper harness adds immutable Lean requirements as the correctness gate. See
[Google DeepMind's AlphaEvolve description](https://deepmind.google/blog/alphaevolve-a-gemini-powered-coding-agent-for-designing-advanced-algorithms/).

Each optimization episode must specify its objectives and demonstrate them with
one or more benchmarks. An objective may target latency, throughput, memory,
execution cost, proof-checking cost, or a combination. The objective specification
names metrics, minimize/maximize direction, constraints, workloads, aggregation,
measurement environment, and the rule for deciding whether a candidate improves
on its parent or baseline. Other objective specifications may supplement this,
but an optimization claim still requires benchmark evidence.

```text
Fixed contract + verified baseline + fixed benchmark definition
                         |
             Select parent implementations
                         |
          Propose implementation and proof changes
                         |
        Check contract integrity and all Lean proofs
                /                        \
        Reject / inconclusive         Accepted
                                          |
                         Build and benchmark the bound artifact
                                          |
                         Retain useful candidates and lineage
                                          |
                     Use improvements as new search parents
                                          |
                       Repeat without deployment review
                                          |
                         Review selected artifact for deployment
```

Correctness is a hard constraint, not a weighted score. No speedup compensates
for a failed or missing theorem. A candidate that changes the contract or evaluator
is ineligible, regardless of its proof or benchmark output.

**Proposed harness behavior:**

- Start optimization from a verified executable baseline. Initial synthesis may
  use the same generator, but cannot claim an improvement before finding one.
- Freeze the objective specification, benchmarks, scoring, and environment for
  an episode. Measure the actual deployment target. Candidate workers cannot
  modify the benchmark harness or submit their own measurements as ground truth.
- Retain verified candidates with parentage, code/proof identities, diagnostics,
  and measured outcomes. Use a population or Pareto frontier when objectives
  compete; the initial selection rule is still to be chosen.
- Bound model usage, candidate count, checker resources, and benchmarking work.
  Preserve the last accepted implementation when the budget is exhausted or no
  improvement is found. Search termination makes no optimality claim.
- Demonstrate improvements using the declared benchmarks, including the
  thresholds and tolerances in the objective. Benchmarks establish measured
  performance, not a theorem about performance for every input.
- Allow accepted, measured improvements to become parents for further search
  immediately, without review. Search acceptance does not authorize deployment.
- Require review of the exact selected artifact before deployment. Search may
  continue while deployment review is pending; that review does not automatically
  authorize deploying descendants. Review is a deployment gate only.

Any implementation satisfying all fixed theorems is acceptable. Optimization is
not required to preserve unspecified behavior or observational equivalence with
its parent. Proof terms may change with each implementation, provided every fixed
requirement is proved again. Required observable behavior belongs in the contract.
Additional testing methods are deferred; benchmarks for optimization are required
now, and existing platform checks remain in place.

### 7. Keep kernel support and app orchestration in their owning repositories

Temper owns the generic specification/artifact interface, proof-checking admission,
verification records, and execution integration. TemperPaw owns app-specific Lean
contracts and implementations, candidate-generating agents, and optimization
episode orchestration. Shared Lean support-library placement remains open.

ADR-0120 provides relevant evolution concepts and immutable evaluator rules, but
the Lean optimizer evolves implementations under a fixed contract. It must not
inherit a spec-mutation workflow that can change that contract. Existing
`temper-evolution` facilities should be assessed for reusable kernel mechanisms;
their presence is not a reason to add app-specific agent logic to this repository.

## Initial Implementation

`temper-lean` implements the immutable package, real Lean proof checker, native
String-to-String ABI, benchmark selection, external-proposer protocol, and exact
artifact review. `temper lean` exposes these operations. Reviewed cores load via
`serve --lean-artifact` and use the existing authenticated Rust service layer.
The complete, small echo fixture demonstrates the end-to-end path. See
[Lean app cores](../lean.md) for package/ABI details, commands, and verification.

The initial policy allows standard logical axioms and pure definitions/proofs,
rejects incomplete proofs and custom execution hooks, and trusts the pinned Lean
compiler/runtime. Benchmarks support latency, throughput, artifact size, and
separately pinned executables for custom objectives. Review is an explicit local
operator action; a public review/upload API is not part of this implementation.

Application-specific contracts and generation agents remain in TemperPaw. Broader
library support, IOA correspondence generation, state-migration proofs, CSDL
generation, and multi-objective selection remain extensions of this interface.
Required app-specific correspondence and migration proofs cannot be inferred from
the kernel's interoperability fixture.

## Rollout Plan

1. **Complete the first component's specification and proofs.** Select a small
   real Lean core and follow Temper's conversation-to-spec workflow. Finish all
   required specification proofs and freeze the verified contract. Specify the
   Rust service interface and benchmark-defined objective. Generate no component
   implementation before its proof-completion gate passes.
2. **Establish checking and native integration.** Pin Lean, dependencies, obligation
   binding, and the trust policy. Prove admission rejects missing obligations and
   changed contracts. Independently establish Rust-to-native-Lean calls with a
   generic interop example; do not generate the selected app component early.
3. **Deliver one verified hybrid app.** Generate the core implementation only
   after the proof-first gate, check its conformance proofs, and connect that
   exact native artifact to Rust services and Temper's data/action surface.
   Exercise persistence, authorization, replay, tenant separation, and any IOA
   correspondence. Require review before deployment.
4. **Optimize under the fixed contract.** Generate implementation/proof pairs,
   check all obligations, and benchmark against the fixed objective. Demonstrate
   that improvements become search parents without review and that deployment
   remains blocked until review. Retain the baseline if no improvement is found.
5. **Grow by fully proved components.** Add functionality incrementally, completing
   each component's proofs before generating its implementation. Extend migration
   support and additional app testing in separate follow-ups.

## Readiness Gates

- Every component's specification proofs are complete before its implementation
  is generated; incomplete scope cannot bypass this gate.
- No implementation or optimizer worker can alter the fixed semantic contract,
  dependency resolution, required obligations, or trusted checking entry point.
- Deliberately broken candidates are rejected: a violated requirement, an omitted
  obligation, a vacuous replacement predicate, a `sorry` dependency, an unsupported
  axiom, and a checked-program/deployed-program mismatch.
- The exported executable is bound to its checked implementation; compiler and
  runtime assumptions are visible in the verification record.
- Hybrid apps execute the checked native Lean core through Rust APIs and services;
  any overlapping IOA behavior has a checked correspondence.
- A real app works through the existing Temper interface and maintains its stated
  properties across persistence/replay and supported upgrades.
- The optimization demonstration uses an unchanged contract and evaluator, and
  records all proof results, the explicit objective, and benchmark measurements.
- Accepted improvements advance search without review, while deployment of each
  selected artifact requires review bound to that artifact.
- Timeout, failed synthesis, failed proof, no improvement, and failed promotion
  leave the current accepted app usable.

## Consequences

### Positive

- Apps can express and prove properties beyond the existing IOA vocabulary.
- Implementation search has a precise acceptance condition independent of the
  generator's approach or model.
- Optimization can change algorithms while preserving the stated behavior.
- Existing IOA apps retain their current path; combined apps can add proofs where
  they provide value.

### Negative

- Authoring an adequate contract and finding proofs can be substantially harder
  than writing a working implementation. Search has no completeness guarantee.
- A Lean toolchain, proof artifacts, and an execution adapter add operational and
  maintenance costs.
- Strict contract immutability requires separate, explicit specification evolution.
- Proof validity does not establish that requirements capture user intent, that
  benchmarks represent production, or that unmodeled host services behave well.

### Risks

- **Under-specification or vacuity:** accepted programs exploit missing properties
  or impossible assumptions. Review admissible inputs, progress, and observable
  behavior alongside safety theorems.
- **Proof/execution mismatch:** compilation, serialization, effects, or candidate
  substitution breaks the connection to proved code. Bind artifacts and specify
  the adapter semantics; retain explicit trust assumptions.
- **Benchmark overfitting:** an implementation improves only the specified
  workloads. Record what they cover and the resulting tradeoffs; broader app
  testing is deferred and no unmeasured performance claim follows.
- **Version incompatibility:** independently valid implementations disagree about
  existing state or replay. Fix representations initially or require compatibility
  and migration proofs before promotion.

### DST Compliance

The native adapter lives in `temper-lean` and `temper-platform`. Any adapter affecting
`temper-runtime`, `temper-jit`, or `temper-server` must run the production path
inside deterministic simulation. Time, identity, randomness, I/O outcomes, and
effect results enter through the simulated host boundary; internal collections
and ordering remain deterministic. The app core must not introduce hidden
threads, ambient I/O, or uncontrolled randomness. Search and proof-checking work
run outside actor dispatch.

Lean proof checking replaces the spec-verification cascade for Lean requirements.
It does not replace DST for the kernel's persistence, routing, replay, effect
handling, and upgrade implementation. Whether those host semantics are also
formalized and proved in Lean is a separate scope decision.

## Non-Goals

- Removing the existing IOA path or requiring existing apps to migrate.
- Mutating the specification to make implementation or optimization succeed.
- Guaranteeing synthesis success, global optimality, or automatic theorem discovery.
- Claiming arbitrary external services or the Rust kernel are verified by an
  application's Lean proofs.
- Implementing app-specific generation agents or evolution orchestration inside
  the Temper kernel repository.

## Alternatives Considered

1. **IOA only.** Retains the existing model but does not provide the requested
   general theorem-based specification and implementation path.
2. **Lean as an advisory check over IOA only.** Adds proof power but cannot express
   Lean-only apps or establish correctness of independently written app code.
3. **Evolve contracts and implementations together.** Useful for deliberate
   product evolution, but violates fixed-contract implementation and optimization.
4. **Use tests or benchmark success as correctness.** Provides observations for
   selected executions rather than proofs of every mandatory proposition.
5. **Require fully verified compilation before any prototype.** Rejected for this
   path: trust the compiler and runtime, and record that execution trust base.
6. **Require review between search generations.** Rejected: review is required
   before deployment, while verified improvements may advance search immediately.

## Rollback Policy

Keep the Lean path opt-in and retain the previous accepted artifact. A failed
optimization does not require deployment. Reverting an implementation requires
compatible persisted state and history, or a verified reverse migration. Preserve
contract revisions, proofs, build identities, and replay provenance for installed
versions. A Lean-only app cannot be rolled back to IOA without separately deriving
and verifying an IOA implementation of its contract.
