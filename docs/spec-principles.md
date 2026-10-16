# Project-specific extension — spec principles

Do not add reusable AI-SDD principles here; read `.claude/skills/shared/sdd-artifact-contract.md` when applying shared specification rules.

Read this before authoring or reviewing canonical specifications and their cases. This manual reference records the existing strict default for this repository; it introduces no new product behavior or verification exemption.

Read `docs/project-config.json` when resolving paths and profiles, and `docs/docs-index-reference.md` when locating a feature owner. Read `.claude/skills/shared/sdd-artifact-contract.md` when applying shared intent, invariant, evidence and M1–M7 gates.

## 1. Profile routing

The current config declares no `specArtifacts` profile. Use the existing strict default: canonical business specifications under `docs/specs`, with §3 stories/acceptance criteria, §4 business rules, §5 domain invariants, §6 interaction intent and §8 canonical cases. Case identities use `TC-{FEATURE}-{NNN}`. One business case may be guarded by several executing tests; do not split an acceptance outcome to mirror helpers.

If a native profile is declared later, validate it before using its owner, roles, identifiers and carriers. An invalid declaration blocks; it is never treated as absent. Do not create a second case registry beside a configured owner.

## 2. Authority and ownership

Accepted user decisions and canonical intent determine expected behavior. Implementation and tests are evidence to compare with that intent, rather than automatic authority over it. Preserve approved scope and record proposed additions separately.

The Windows capability owner is `docs/specs/windows-docker-ui.md`; its accepted scope and design companion are `plans/261014-1121-windows-docker-ui/plan.md`. The current draft describes planned behavior, rather than shipped Windows support. Read those owners when changing Windows resource targeting, transfers, terminals or delivery.

Keep shared semantic rules in the shared SDD contract. Keep repository paths, commands and actual ownership here or in the project config. Read `docs/domain-entities-reference.md` when changing live-resource lifecycle or application choices, and `docs/integration-test-reference.md` when selecting executable boundary proof.

## 3. Prose and evidence

### 3.1 Narrative and carriers

Behavioral prose states the actor, intent, observable outcome, constraints and domain ownership in technology-independent terms. Describe interface roles, navigation, observable states and per-story action flows; put visual detail in the linked design companion.

Implementation identifiers belong in explicit evidence carriers: frontmatter, `[Source: ...]`, `Evidence`, `CoveredBy` and structured diagrams. `IntegrationTest` is legacy mapping input. Review reports may cite physical paths and line numbers. A matching identifier or successful aggregate result alone does not prove an assertion executed.

### 3.2 Banned prose tokens

No project-specific token verifier or additional banned-token list is declared. Apply the shared M1/M2 technology-independent prose rule to business narrative: framework, programming-language, product, database, transport and implementation-pattern names belong in the declared carriers, rather than behavioral rules or scenarios. Do not extend this prose restriction to evidence carriers or technical reference documentation. Unknown profile or carrier rules remain explicit gaps.

## 4. Case and test mapping

Read `.claude/skills/shared/tc-format.md` when authoring strict-default cases. Preserve owner-qualified case identity and real variant identity. Every claimed tested outcome must resolve to an actual executor and inspected intent-sensitive assertion, or an explicitly accepted manual evidence carrier.

Cases protect user promises and invariants across applicable success, error, boundary, authorization, preservation and lifecycle states. Universal hard rules need quantified input domains, invariants and boundary counter-inputs. Arrange observable starting/concurrent states before acting; a fixed sleep does not prove the required state.

Planned, untested and blocked evidence remains labeled accordingly. Native Windows behavior requires native evidence; a successful Mac compile, mock or command-line smoke does not prove installed Windows behavior.

## 5. Verification and derived artifacts

The active skill/workflow owns verification order and review budgets. This reference adds or removes no gate. Read `docs/code-review-rules.md` when selecting existing quality checks, and `docs/integration-test-reference.md` when interpreting prerequisite-sensitive results.

Reconcile intent, cases, executing assertions and documentation when behavior changes. Regenerate an actual generated projection through its declared owner; never infer a generator from a filename. No additional spec verifier or generated case index is configured here.

Keep disposable reports, logs and qualification results under `tmp/`; canonical intent stays at its owner. Do not store credentials or private fixture data in specifications or reports.

## Closing reminders

Validate config and resolve the real owner first. Preserve the accepted scope, technology-independent narrative and one-case-to-many-tests relation. Read and verify the actual assertion before claiming coverage. Planned behavior and unavailable native proof never become a passing support claim.
