# Project structure and execution

> Last scanned: 2026-10-14

Read this before planning cross-module changes, locating executable entry points or choosing build/runtime prerequisites. The repository has one frontend package and one shared Rust application crate; source directories do not establish independently deployed services.

## Entry points and source owners

| Surface | Entry and boundary |
| --- | --- |
| Svelte UI | `index.html:11` → `src/main.ts:12` → `src/App.svelte:11`. One shell statically imports views; page selection is rune state. Terminal delays first mount and then hides its instance to retain PTYs (`src/App.svelte:75`, `src/App.svelte:154`, `src/App.svelte:193`). |
| Native application | `src-tauri/src/main.rs:4` calls `colima_ui_lib::run`; setup registers plugins/state/commands and starts API/polling/watchers (`src-tauri/src/lib.rs:72`, `src-tauri/src/lib.rs:93`, `src-tauri/src/lib.rs:142`). |
| HTTP API | Started from native setup; router is `src-tauri/src/api_server.rs:53`. Health is public, operations are protected. Packaged router does not serve frontend assets; browser UI is the Vite development surface (`src-tauri/src/lib.rs:101`, `src-tauri/src/api_server.rs:288`, `src-tauri/src/api_server.rs:298`, `src-tauri/src/api_server.rs:354`). |
| `colimaui` CLI | Explicit Cargo binary requires feature `cli`; main builds a Tokio runtime and dispatches to shared library services (`src-tauri/Cargo.toml:71`, `src-tauri/src/bin/cli.rs:216`, `src-tauri/src/bin/cli.rs:223`, `src-tauri/src/bin/cli.rs:274`). |
| `cui` CLI | Async source entry reads the local credential reference, discovers the running API by health and sends authenticated `/api/cli/chat` requests (`src-tauri/src/bin/cui.rs:90`, `src-tauri/src/bin/cui.rs:118`, `src-tauri/src/bin/cui.rs:127`, `src-tauri/src/bin/cui.rs:157`). |

Frontend behavior owners are pages/components, rune stores and typed modules under `src/lib/api/`; `src/lib/api.ts` is a compatibility facade. CSS enters through the imports-only `src/index.css`. Rust's shared crate declares commands/routes, services/adapters and infrastructure modules; request paths do not universally traverse every layer (`src/lib/api.ts:1`, `src/lib/api/index.ts:1`, `src/index.css:1`, `src-tauri/src/lib.rs:1`, `src-tauri/src/commands/containers.rs:242`, `src-tauri/src/services/container.rs:41`).

## Runtime and external boundaries

Ordinary UI operations select IPC or localhost HTTP through the shared API client. Dashboard push uses Tauri events or browser SSE; metrics use SSE in both modes. Interactive terminals use native IPC Channel/PTY and have no HTTP terminal-session surface; sandbox execution routes remain separate. Frontend AI streaming can call provider endpoints directly through browser/Tauri-plugin fetch, separately from local resource APIs (`src/lib/api/client.ts:103`, `src/lib/dataPoller.ts:145`, `src/lib/api/metrics.ts:65`, `src/lib/terminal-transport.ts:80`, `src-tauri/src/api_server.rs:277`, `src-tauri/src/api_server.rs:284`, `src/lib/llmProviders.ts:20`).

Docker/nerdctl, Colima, Lima and Kubernetes operations cross local process/daemon boundaries. Runtime helpers resolve active profile/runtime and PATH; service adapters are also used by CLI/self-heal. Knowledge-bank SQLite and independent activity SQLite store application choices/history; DockerState caches external live state. Transfer registry and child-process registry are separate in-memory owners (`src-tauri/src/commands/runtime.rs:3`, `src-tauri/src/services/container.rs:9`, `src-tauri/src/commands/knowledge_bank.rs:19`, `src-tauri/src/commands/activity.rs:293`, `src-tauri/src/docker_state.rs:142`, `src-tauri/src/transfer_registry.rs:1`).

Vite fixes port 1420 with strict-port behavior; optional TAURI_DEV_HOST changes host/HMR configuration, whose port is 1421. Tauri dev uses that URL; build consumes `../dist`. The API binds an available localhost port in 11420–11429, discovered by clients (`vite.config.ts:42`, `src-tauri/tauri.conf.json:6`, `src-tauri/src/api_server.rs:319`, `src/lib/api/client.ts:45`).

Read `docs/backend-patterns-reference.md` for request/storage/background owners, `docs/frontend-patterns-reference.md` for state/navigation, and `docs/domain-entities-reference.md` for resource identities and invariants.

## Toolchain declarations

Node is declared `>=22.22.2`; npm package is private ESM. Manifest ranges include Svelte `^5.56.8`, TypeScript `~6.0.3`, Vite `^8.1.5` and Tauri JS `^2`. Rust edition is 2021, with Tauri `2`, Axum `0.8` and Tokio `1`. These are declarations, not installed-version claims (`package.json:2`, `package.json:7`, `package.json:30`, `package.json:54`, `src-tauri/Cargo.toml:1`, `src-tauri/Cargo.toml:24`, `src-tauri/Cargo.toml:28`).

Version identities are separate: npm/Cargo declare 0.2.0 while Tauri config declares 0.1.10; read their owners before changing release identity (`package.json:5`, `src-tauri/Cargo.toml:3`, `src-tauri/tauri.conf.json:4`). Deep-link plugin/scheme registration alone does not prove an implemented OAuth handoff (`src-tauri/src/lib.rs:89`, `src-tauri/tauri.conf.json:39`).

## Build, quality and delivery

Use `npm run dev` for Vite, `npm run tauri dev` for native development, `npm run build` for frontend output and `npm run tauri build` for desktop bundles. Node/npm/Cargo forms are shared across hosts, but native Tauri prerequisites must match the target OS; release configuration currently targets macOS ARM64/x86_64 and Linux, not a Windows release lane (`package.json:10`, `src-tauri/tauri.conf.json:6`, `.github/workflows/ci.yml:80`, `.github/workflows/release.yml:92`).

CI runs on main/staging/dev pushes and PRs. Frontend lint/type/Svelte/token/i18n/Vitest gates run on Ubuntu; Rust check/Clippy uses macOS/Linux matrix, with `cargo test --lib` only on native Linux. This does not execute external files under `src-tauri/tests`. Read `docs/code-review-rules.md` for exact gates and `docs/integration-test-reference.md` for deliberately selected boundary tests (`.github/workflows/ci.yml:3`, `.github/workflows/ci.yml:18`, `.github/workflows/ci.yml:52`, `.github/workflows/ci.yml:104`).

Manual promotion creates dev→staging or staging→main PRs. Version tags select beta/RC/stable releases; stable requires main ancestry, RC staging ancestry only warns, beta has no branch restriction. Release asset checks warn without failing, so they do not prove artifact readiness. No rollback contract was established from these owners (`.github/workflows/promote.yml:6`, `.github/workflows/release.yml:33`, `.github/workflows/release.yml:66`, `.github/workflows/release.yml:223`).

Read `scripts/release.sh` only when preparing authorized publication: it changes versions, commits, tags and pushes. Updater settings belong to Tauri config; CI signing uses GitHub secret references, including TAURI_SIGNING_PRIVATE_KEY and optional Apple signing/notarization references. Record names/mechanisms, never values (`scripts/release.sh:29`, `scripts/release.sh:90`, `src-tauri/tauri.conf.json:46`, `.github/workflows/release.yml:170`).

## Configuration and readiness

Runtime discovery reads PATH/HOME, prioritizes nonempty DOCKER_HOST and checks COLIMA_HOME/HOME socket locations. Application data uses HOME or `/tmp` fallback plus `.colima-ui`; it is not the owner of every export or OS login item. Browser auth consumes an out-of-band fragment credential; native gets it via IPC. Do not copy credential content into docs/evidence (`src-tauri/src/path_util.rs:65`, `src-tauri/src/path_util.rs:73`, `src-tauri/src/path_util.rs:195`, `src/lib/api/client.ts:179`, `src/lib/api/client.ts:194`).

API port exhaustion logs API-disabled rather than aborting the desktop process. Health proves liveness/discovery, not Docker/Colima readiness. Resource capabilities and actual operation prerequisites must be checked separately (`src-tauri/src/api_server.rs:387`, `src-tauri/src/routes/system.rs:203`, `src/store/capabilities.svelte.ts:63`). Read `README.md` for setup, and `docs/telemetry.md` before changing outbound data.
