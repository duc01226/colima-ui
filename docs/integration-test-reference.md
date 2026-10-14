# Boundary testing

> Last scanned: 2026-10-14

Read this before selecting or changing boundary tests. Choose by the interaction and final outcome actually exercised: a folder name, ignored test or successful early return does not prove live integration. This guide was derived from source; tests were not run during the scan.

## Harness and commands

Rust tests call `colima_ui_lib` owners directly. Files under `src-tauri/tests` exercise Docker/Trivy/host boundaries; in-source library tests also exercise real Axum, SQLite, filesystem and child-process boundaries. Vitest uses Svelte/jsdom, jest-dom setup and vmThreads isolation, with explicit network/native substitutes (`src-tauri/tests/file_transfer_against_docker.rs:14`, `src-tauri/src/api_server.rs:403`, `src-tauri/src/commands/activity.rs:507`, `src-tauri/src/streaming_cmd.rs:544`, `vitest.config.ts:4`, `vitest.config.ts:12`, `src/setupTests.ts:1`).

Use package scripts for frontend checks (`npm test`); use `cargo test --manifest-path src-tauri/Cargo.toml --lib` for library tests. CI runs the latter only on native Linux, and does not run external integration files (`package.json:15`, `.github/workflows/ci.yml:104`). Cargo/npm commands have the same syntax across hosts, but the selected test may require Unix tools/native Tauri prerequisites; these commands alone do not establish Windows compatibility.

Live suites are deliberately ignored. The documented Docker-event, transfer and collector-lifecycle commands opt in and serialize shared state. From repository root, an example is:

```text
cargo test --manifest-path src-tauri/Cargo.toml --test file_transfer_against_docker -- --ignored --test-threads=1
```

The manifest location and test flags come from repository owners; inspect the chosen file's module docs before running a different suite (`package.json:19`, `src-tauri/tests/file_transfer_against_docker.rs:7`, `src-tauri/tests/docker_events_against_docker.rs:6`, `src-tauri/tests/metrics_collector_lifecycle.rs:10`). These tests create/kill local resources, so select their stated prerequisites and scope deliberately.

## Boundary intent and meaningful assertions

| Boundary | Representative owner and owned outcome |
| --- | --- |
| Docker event conversion | Real daemon crash loop produces separate die events with exit codes, observed under a 60-second deadline; readiness alone is insufficient (`src-tauri/tests/docker_events_against_docker.rs:64`, `src-tauri/tests/docker_events_against_docker.rs:94`, `src-tauri/tests/docker_events_against_docker.rs:104`). |
| Background transfer | Docker accepts app-exported archives; copy round trip checks readable TAR/size/hash. Registry cases check terminal state without event subscription, conflict refusal and cancel cleanup, including absent partial output/orphan process (`src-tauri/tests/file_transfer_against_docker.rs:103`, `src-tauri/tests/file_transfer_against_docker.rs:162`, `src-tauri/tests/file_transfer_against_docker.rs:265`, `src-tauri/tests/file_transfer_against_docker.rs:301`, `src-tauri/tests/file_transfer_against_docker.rs:469`). |
| Trivy/runtime | Real scan asserts image digest/scanner/database provenance; cache identity and absent-local-image refusal are separate cases. Corpus scoring tests local machine inputs, not a universal image ordering (`src-tauri/tests/security_scan_against_trivy.rs:42`, `src-tauri/tests/security_scan_against_trivy.rs:70`, `src-tauri/tests/security_scan_against_trivy.rs:88`, `src-tauri/tests/security_score_against_corpus.rs:70`). |
| Router/auth/SSE | Real `build_router().oneshot` tests public/protected routes and token rules. Holding/dropping response body checks subscriber lifetime without a listening socket (`src-tauri/src/api_server.rs:403`, `src-tauri/src/api_server.rs:429`, `src-tauri/src/api_server.rs:478`). |
| SQLite/FTS | Activity helpers use production schema and assert stored redaction/read-back/retention. Article tests use a handwritten relevant schema and assert no duplicates, same-version preservation and FTS results; they do not prove full startup migration (`src-tauri/src/commands/activity.rs:507`, `src-tauri/src/commands/activity.rs:621`, `src-tauri/src/commands/activity.rs:647`, `src-tauri/src/commands/kb_articles.rs:338`, `src-tauri/src/commands/kb_articles.rs:375`, `src-tauri/src/commands/kb_articles.rs:468`). |
| Filesystem/process | Colima config round trip preserves unmanaged YAML, original-on-error and prior-content backup. Streaming tests check bytes, child cancellation, partial cleanup and destination preservation (`src-tauri/src/commands/colima_config.rs:796`, `src-tauri/src/commands/colima_config.rs:885`, `src-tauri/src/commands/colima_config.rs:1064`, `src-tauri/src/streaming_cmd.rs:583`, `src-tauri/src/streaming_cmd.rs:602`, `src-tauri/src/streaming_cmd.rs:646`). |
| Frontend handoff/lifecycle | Transfer dialog uses real component/store with mocked API; notification wiring uses real store with substituted transport/listing/OS notification. Data-poller tests use fake timers/EventSource for disposal/restart. They prove those module contracts, not live IPC/HTTP/Docker (`src/components/transfer/TransferDialog.test.ts:20`, `src/components/transfer/TransferDialog.test.ts:118`, `src/lib/transferNotifications.test.ts:4`, `src/lib/transferNotifications.test.ts:155`, `src/lib/dataPoller.test.ts:20`, `src/lib/dataPoller.test.ts:83`). |

Observe the system-owned result: artifact contents, persisted row, resource absence, terminal status, subscriber lifetime or rendered/store state. Existing transfer helpers use bounded condition polling; Docker events use a stream deadline; collector lifecycle uses fixed settling waits; UI uses `waitFor` and fake timers (`src-tauri/tests/file_transfer_against_docker.rs:64`, `src-tauri/tests/docker_events_against_docker.rs:95`, `src-tauri/tests/metrics_collector_lifecycle.rs:20`, `src/components/transfer/TransferDialog.test.ts:128`, `src/lib/dataPoller.test.ts:80`).

## Prerequisites, isolation and cleanup

Transfer suites assume a running Docker daemon and named local fixture images; selected cases also use shasum, ps, dd, tar and pgrep. They have process-global registries and fixed per-case temp tags, requiring serial deliberate execution; those tags do not isolate simultaneous independent processes. WorkDir has Drop cleanup, while container cleanup varies between inline removal and `catch_unwind` followed by removal. Inspect setup failures too (`src-tauri/tests/file_transfer_against_docker.rs:1`, `src-tauri/tests/file_transfer_against_docker.rs:29`, `src-tauri/tests/file_transfer_against_docker.rs:80`, `src-tauri/tests/file_transfer_against_docker.rs:165`, `src-tauri/tests/file_transfer_against_docker.rs:189`, `src-tauri/tests/file_transfer_against_docker.rs:415`, `src-tauri/tests/file_transfer_against_docker.rs:508`).

SQLite exemplars allocate fresh in-memory connections. Activity's failure case also calls the real `record` path, so the module is not universally isolated from runtime storage. Config fixtures pass explicit PID+counter scratch homes with Drop cleanup; crash fixtures use fixed tags. Streaming-process cases serialize shared registry/kill-all state with a module mutex and use real shell tools (`src-tauri/src/commands/activity.rs:593`, `src-tauri/src/commands/kb_articles.rs:338`, `src-tauri/src/commands/colima_config.rs:737`, `src-tauri/src/crash.rs:265`, `src-tauri/src/streaming_cmd.rs:523`, `src-tauri/src/streaming_cmd.rs:551`).

CLI/storage setup is appropriate when it prepares state for the boundary under test. For example, directory copy seeds contents with Docker CLI before testing app copy-out. Frontend tests reset state/mocks and stop subscriptions/unmount; preserve that ownership when adding cases (`src-tauri/tests/file_transfer_against_docker.rs:415`, `src/components/transfer/TransferDialog.test.ts:42`, `src/lib/transferNotifications.test.ts:60`).

## Interpret results and evidence limits

- Trivy cases require tool/runtime readiness and a downloaded scanner DB, but preflight only probes Trivy/Docker. Missing prerequisites print skip and return successfully. Corpus cases also return for insufficient local images, then continue individual failures before requiring enough scored images (`src-tauri/tests/security_scan_against_trivy.rs:1`, `src-tauri/tests/security_scan_against_trivy.rs:28`, `src-tauri/tests/security_score_against_corpus.rs:49`, `src-tauri/tests/security_score_against_corpus.rs:103`).
- Metrics CLI-parser comparison permits an empty daemon; API-vs-CLI requires nonempty shared-container samples. The ignored release cost case provisions a fleet and asserts tick count plus CPU; it is a real-machine measurement, not deterministic unit coverage (`src-tauri/tests/metrics_collector_lifecycle.rs:76`, `src-tauri/tests/metrics_collector_lifecycle.rs:119`, `src-tauri/tests/metrics_collector_cost.rs:14`, `src-tauri/tests/metrics_collector_cost.rs:135`).
- Structural activity-recording and invoke-key tests check maintained source text. Mocked AI persistence tests check store→API calls. They do not prove a resource command executed, actual Tauri IPC, or disk persistence (`src-tauri/tests/activity_records_every_mutating_command.rs:3`, `src/lib/api/invoke-args.test.ts:20`, `src/store/ai-persistence.test.ts:10`).

Read `docs/seed-test-data-reference.md` when changing fixture/corpus/bootstrap content, and `docs/code-review-rules.md` when deciding the required quality gates. Record whether the boundary assertions actually ran; distinguish skipped prerequisites, substituted seams and structural sensors from runtime outcomes.
