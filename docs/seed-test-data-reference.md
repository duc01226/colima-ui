# Seed and fixture setup

> Last scanned: 2026-10-14

Read this before changing sample data, fixture loading or startup knowledge content. Keep synthetic evaluation data, frozen test fixtures and the user's runtime database separate.

## Owners and entry points

| Data purpose | Owner and invocation | Scope and repeatability |
| --- | --- | --- |
| Broken Compose evaluation cases | `scripts/build-compose-corpus.py:21`; its `main()` at line 184 writes `tests/compose-corpus` | Regenerates named case files and manifest; synthetic cases are not proof about real user files. |
| Dockerfile parser fixtures | `src-tauri/src/commands/dockerfile_parse.rs:182` reads `tests/dockerfile-corpus` | Requires byte-identical parse/render round trips and at least 20 checked files; it does not build images (`src-tauri/src/commands/dockerfile_parse.rs:193`, `src-tauri/src/commands/dockerfile_parse.rs:203`). |
| Scanner parser fixtures | `src-tauri/src/commands/security_scan.rs:782` reads `tests/fixtures` relative to the Cargo crate working directory (`src-tauri/tests/fixtures` in the repository) | Frozen JSON protects parsing outcomes; a fixture pass is not a live vulnerability scan. |
| Builtin troubleshooting solutions | `src-tauri/src/commands/knowledge_bank.rs:185` calls `seed_builtins` when no builtin solution rows exist | Runtime bootstrap of the user's application store, not a test-only seeder. |
| Offline Help articles | `src-tauri/src/commands/kb_articles.rs:169` seeds compiled article bodies during knowledge-bank initialization | Upsert by slug/locale and newer article version; same-version rows stay intact. |

## Runtime data ownership

Application setup initializes the knowledge bank before starting the HTTP API (`src-tauri/src/lib.rs:97`). Its SQLite file is `knowledge.db` under the application data directory (`src-tauri/src/commands/knowledge_bank.rs:19`). The directory comes from nonempty `HOME` or the `/tmp` fallback, followed by `.colima-ui` (`src-tauri/src/path_util.rs:65`). Treat it as user data; there is no test-environment guard on this normal application startup path.

Builtin solutions use a whole-set empty check. `seed_builtins` issues ordinary inserts and ignores individual insert errors, so this mechanism does not promise row-by-row reconciliation of a partial set or a seed transaction (`src-tauri/src/commands/knowledge_bank.rs:185`, `src-tauri/src/commands/knowledge_bank.rs:416`).

Help article bodies are compiled with literal `include_str!` paths, then copied into SQLite for search (`src-tauri/src/commands/kb_articles.rs:9`, `src-tauri/src/commands/kb_articles.rs:68`). Bump `ARTICLE_VERSION` when content changes. The unique slug/locale key and greater-version condition govern replacement; a failed row is logged without aborting startup (`src-tauri/src/commands/knowledge_bank.rs:124`, `src-tauri/src/commands/kb_articles.rs:30`, `src-tauri/src/commands/kb_articles.rs:169`).

The article tests create an in-memory database and assert that repeated seeding adds no duplicates and same-version reseeding preserves an edited row. Reuse that isolation pattern when testing this contract (`src-tauri/src/commands/kb_articles.rs:338`, `src-tauri/src/commands/kb_articles.rs:375`).

## Evaluation and fixture safety

The Compose generator overwrites each declared case and `manifest.json`; inspect changes before retaining regenerated fixtures. Its cases explicitly carry synthetic provenance, and the generator does not establish real-world fix accuracy (`scripts/build-compose-corpus.py:2`, `scripts/build-compose-corpus.py:184`).

The Compose benchmark reads input YAML through Docker's validator and writes a CSV. It requires Docker Compose and distinguishes detection from useful advice; it does not modify input YAML (`scripts/compose-diagnose-benchmark.sh:9`, `scripts/compose-diagnose-benchmark.sh:28`, `scripts/compose-diagnose-benchmark.sh:44`, `scripts/compose-diagnose-benchmark.sh:83`).

The scanner bakeoff uses existing local Docker images and can download/update scanner databases. Its output directory is caller-selected; use a disposable workspace directory for a new run. It is an environment-dependent benchmark, not a hermetic fixture generator (`scripts/security-scanner-bakeoff.py:20`, `scripts/security-scanner-bakeoff.py:52`, `scripts/security-scanner-bakeoff.py:111`).

This reference was derived from source. No loaders or benchmarks ran, no user database was read, and no runtime test result or accepted baseline is claimed. Read `docs/integration-test-reference.md` when changing boundary-test setup.
