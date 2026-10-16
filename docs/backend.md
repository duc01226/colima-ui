# Backend Reference

The Rust backend (~33,700 lines across 101 files) is organized into layers: **commands** (business logic), **routes** (HTTP delegation), **adapters** (CLI abstraction), and **services** (orchestration).

## Module Map

```
src-tauri/src/
├── lib.rs                 # App setup, plugin registration, command handler registration
├── main.rs                # Entry point (calls lib::run())
├── api_server.rs          # Axum router + HTTP startup (ports 11420–11429)
├── auth.rs                # API token generation + Bearer auth middleware
├── api_reference.rs       # Private exact-instance credential/port handoff for cui
├── private_file.rs        # Owner-only creation and opened-handle verification
├── windows_host.rs        # Windows Known Folders and fallible native host facts
├── windows_pty.rs         # Native ConPTY, atomic Job membership and bounded I/O
├── host_features.rs       # Native host-feature policy and raw HTTP/tool refusal
├── engine_connection.rs   # Captured Windows target, original identity and read generations
├── child_process.rs       # Generic bounded capture and native child-tree ownership
├── app_settings.rs        # Existing settings-row SQL and typed read failures
├── sse.rs                 # SSE broadcast, Docker watcher, instance publisher
├── validation.rs          # Security: shell injection, banned flags, ID validation
├── helpers.rs             # ApiResponse, ok/err, run_blocking, run_cmd, TimedCache
├── platform.rs            # OS/arch/package manager detection
├── docker_state.rs        # Bollard Docker event stream for real-time push
├── instance_reader.rs     # Colima instance YAML config parser (fast filesystem read)
├── terminal_session.rs    # PTY-based terminal session for xterm.js
├── poller.rs              # Background instance status poller
├── path_util.rs           # Shared data root, native PATH and executable discovery
├── commands/              # Business logic (41 modules)
├── routes/                # HTTP route handlers (25 modules)
├── adapters/              # Unified DevOps adapter traits (7 modules)
└── services/              # High-level orchestration (4 modules)
```

## Commands Layer (`commands/`)

The central business logic layer. Both Tauri IPC and HTTP routes call into these functions.

| Module | Lines | Responsibility |
|--------|-------|---------------|
| `containers.rs` | 966 | Docker container & image CRUD, stats, exec, run, prune |
| `knowledge_bank.rs` | 1001 | SQLite knowledge bank: solutions, feedback, memory, settings, presets |
| `searxng.rs` | 628 | SearXNG/DuckDuckGo web search + HTML→Markdown conversion |
| `colima.rs` | 730 | Colima instance lifecycle + diagnostics + worker nodes |
| `ai_chat.rs` | 862 | Multi-provider AI chat (Anthropic/OpenAI/Google/Ollama/...) |
| `kubernetes.rs` | 644 | kubectl operations: pods, deployments, services, namespaces, events |
| `system.rs` | 598 | System info, tool checks, host specs, resource saver mode |
| `shell_sandbox.rs` | 291 | 3-tier command sandbox: safe/approve/banned classification |
| `lima.rs` | 285 | Lima VM lifecycle + shell + templates + create |
| `models.rs` | 200 | Ollama model management (list, pull, serve, delete) |
| `compose.rs` | 156 | Docker Compose project management |
| `networks.rs` | 166 | Docker network CRUD + prune |
| `volumes.rs` | 178 | Docker volume CRUD + prune |
| `agent_loop.rs` | 135 | AI agent tool execution loop |
| `runtime.rs` | 159 | Runtime detection (docker vs nerdctl) |
| `self_heal.rs` | 1122 | Self-healing rules & kill switch (no route performs a repair) |
| `colima_config.rs` | 1101 | Read/write Colima instance configuration |
| `file_transfer.rs` | 1067 | Container copy & image TAR transfer jobs |
| `security_scan.rs` | 942 | Trivy scan orchestration |
| `security_rules.rs` | 780 | Configuration rule pack |
| `activity.rs` | 757 | Activity record: what was done to the machine |
| `diagnostics.rs` | 680 | Diagnostic bundle collection |
| `metrics_collector.rs` | 667 | The app's only sampling loop; Pro plugs in via `set_metric_writer` |
| `k8s_cluster.rs` | 632 | Cluster health, nodes, component status |
| `activity_feed.rs` | 577 | Feed assembly and export (JSON/CSV) |
| `kb_articles.rs` | 482 | Bundled knowledge-base articles (5 locales) |
| `compose_diagnose.rs` | 473 | Compose file diagnosis (schema, undefined refs, YAML) |
| `topology.rs` | 465 | Topology graph endpoint |
| `security_score.rs` | 415 | Score computation with scanner/db provenance |
| `k8s_resources.rs` | 308 | Generic K8s resource browsing & CRDs |
| `system_capabilities.rs` | 276 | Capability reporting |
| `dockerfile_parse.rs` | 270 | Dockerfile parsing for the layer/security views |
| `security_catalog.rs` | 248 | Base-image alternatives catalog (local table) |
| `announcements.rs` | 239 | Release notes & advisories feed (fetch + filter) |
| `compose_services.rs` | 232 | Per-service compose inspection |
| `engine_resources.rs` | 232 | Aggregate engine CPU/memory (one-shot, expensive) |
| `autostart.rs` | 217 | Launch-at-login toggle |
| `activity_coverage.rs` | 182 | Asserts every mutating command records activity |
| `terminal.rs` | 126 | PTY terminal command surface |
| `kind.rs` | 63 | Kind cluster lifecycle |

### Key Patterns

**Tauri commands** use `#[tauri::command]` and are registered in `lib.rs`:
```rust
#[tauri::command]
pub async fn start_container(container_id: String) -> Result<String, String> {
    // Security validation
    if !crate::validation::is_valid_container_id(&container_id) {
        return Err("Invalid container ID format".to_string());
    }
    // Execute via CLI
    tokio::task::spawn_blocking(move || {
        docker_output(&["start", &container_id])
    }).await.map_err(|e| format!("Task error: {}", e))?
}
```

**CLI-only variants** (`_cli` suffix) exist for operations where the Tauri command requires state (`tauri::State`) that HTTP routes cannot provide:
- `list_containers_cli()` — No Bollard/DockerState, falls back to `docker ps`
- `stop_instance_cli()` / `delete_instance_cli()` — No Docker state reconnect

## Routes Layer (`routes/`)

Thin HTTP handlers that extract request parameters and delegate to `commands/`:

| Module | Lines | Endpoints |
|--------|-------|-----------|
| `k8s.rs` | 832 | 30+ Kubernetes endpoints (complex kubectl logic) |
| `payloads.rs` | 896 | All request/response struct definitions |
| `system.rs` | 217 | System info, tool checks, install deps, prune |
| `containers.rs` | 171 | Container CRUD, logs, stats, exec, run |
| `ai.rs` | 255 | AI chat, CLI chat, tool execution |
| `kb.rs` | 184 | Knowledge bank queries, feedback, memory |
| `images.rs` | 87 | Image CRUD, pull, prune |
| `lima.rs` | 77 | Lima VM operations |
| `instances.rs` | 69 | Colima instance management |
| `compose.rs` | 76 | Docker Compose operations |
| `networks.rs` | 53 | Network CRUD |
| `volumes.rs` | 54 | Volume CRUD |
| `models.rs` | 45 | Ollama model management |
| `misc.rs` | 221 | SSE events endpoint |
| `security.rs` | 158 | Security scan/score/rules endpoints |
| `colima_config.rs` | 97 | Read/write Colima instance configuration |
| `file_transfer.rs` | 90 | Container copy & image TAR transfer jobs |
| `self_heal.rs` | 84 | Self-healing rules & kill switch (no route performs a repair) |
| `activity.rs` | 68 | Activity record: what was done to the machine |
| `capabilities.rs` | 35 | Capability probe endpoints |
| `diagnostics.rs` | 31 | Diagnostic bundle collection |
| `announcements.rs` | 21 | Release notes & advisories feed (fetch + filter) |
| `system_capabilities.rs` | 17 | Capability reporting |
| `topology.rs` | 15 | Topology graph endpoint |

### Delegation Pattern

```rust
// routes/containers.rs — thin delegation
pub async fn api_start_container(
    Query(q): Query<ContainerIdQuery>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    match containers::start_container(q.container_id).await {
        Ok(out) => ok(out),
        Err(e) => err(e),
    }
}
```

## Adapters Layer (`adapters/`)

Trait-based abstraction over CLI tools for future testability:

| Module | Lines | Abstraction |
|--------|-------|-------------|
| `traits.rs` | 270 | `ContainerRuntime`, `VmManager`, `Orchestrator` traits |
| `docker.rs` | 372 | Docker CLI adapter |
| `nerdctl.rs` | 372 | nerdctl CLI adapter |
| `colima.rs` | 252 | Colima CLI adapter |
| `lima.rs` | 192 | Lima CLI adapter |
| `compose.rs` | 58 | Docker Compose adapter |
| `kubectl.rs` | 55 | kubectl adapter |

## Services Layer (`services/`)

Higher-level orchestration (currently lightweight):

| Module | Lines | Purpose |
|--------|-------|---------|
| `container.rs` | 104 | Container operations via adapter traits |
| `vm.rs` | 60 | VM operations via adapter traits |
| `compose.rs` | 28 | Compose operations via adapter traits |
| `orchestration.rs` | 28 | Cross-cutting orchestration |

## Core Infrastructure

### api_server.rs
- Builds the Axum router with all route groups; auth precedes the Windows unsupported-host guard (`src-tauri/src/api_server.rs:366`)
- Native `api_port` IPC returns only the bound instance’s port or an error; browser health discovery remains separate (`src-tauri/src/api_server.rs:34`)
- Applies CORS (localhost origins), auth middleware, and CSP headers
- Tries localhost ports 11420–11429 in a background tokio task; publishes the private CLI reference only after binding (`src-tauri/src/api_server.rs:391`, `src-tauri/src/api_server.rs:417`)
- Private-reference publication failure preserves in-app access; public health and existing IPC/browser-fragment authentication retain their contracts. Read `docs/architecture.md` when changing credential handoff.

### Host data, discovery and private files

`path_util` owns the shared app/CLI data root and native PATH split/join. Windows uses the account’s LocalAppData Known Folder plus `ColimaUI`; macOS/Linux retain `HOME/.colima-ui` with the existing `/tmp` fallback. Windows executable discovery resolves `.exe` files from absolute search directories; POSIX shell/PATH augmentation stays in its platform branch (`src-tauri/src/path_util.rs:76`, `src-tauri/src/path_util.rs:102`, `src-tauri/src/path_util.rs:150`, `src-tauri/src/path_util.rs:240`, `src-tauri/src/windows_host.rs:38`).

Windows tool-version probes resolve the executable fallibly and use bounded owned capture (`src-tauri/src/commands/system.rs:52`). CPU, memory and disk probes return errors at `windows_host`; IPC/HTTP host specs, diagnostic sections and Colima configuration callers preserve failures. Crash reporting also uses fallible data resolution and falls back to redacted stderr if storage cannot be resolved (`src-tauri/src/commands/system.rs:413`, `src-tauri/src/routes/system.rs:68`, `src-tauri/src/commands/diagnostics.rs:155`, `src-tauri/src/commands/colima_config.rs:589`, `src-tauri/src/commands/colima_config.rs:628`, `src-tauri/src/crash.rs:202`).

Windows prerequisite metadata is executable-only: HTTP `load_system_info` and native `check_system` share `system::system_info`, so a usable Docker CLI does not need a selected/healthy engine to be reported. Connection readiness remains a separate owner. POSIX HTTP retains its existing direct probes and formatting (`src-tauri/src/helpers.rs:220`, `src-tauri/src/commands/system.rs:69`, `src-tauri/src/commands/system.rs:156`).

`api_reference` stores only the nonsecret instance marker in existing `app_settings`. `cui` reads it without creating a database, then validates the matching owner-private port/credential file; it never falls back to public health discovery. `private_file` creates owner-only access before bytes and verifies the opened handle on both write and read (`src-tauri/src/api_reference.rs:72`, `src-tauri/src/api_reference.rs:93`, `src-tauri/src/private_file.rs:8`, `src-tauri/src/bin/cui.rs:97`). These source contracts do not establish native Windows runtime qualification.

### Engine connection, processes and settings

`engine_connection` captures a resolved Windows context/pipe/executable/config, controls Docker environment/global selectors, and verifies the original Linux daemon before CLI/SDK use. Loss/replacement and overlapping checks use identity/generation guards; saved preferences are next-launch input. `runtime::run` and short helpers use generic bounded capture, while `app_settings` centralizes existing settings reads/upserts without a new schema (`src-tauri/src/engine_connection.rs:20`, `src-tauri/src/engine_connection.rs:136`, `src-tauri/src/engine_connection.rs:354`, `src-tauri/src/commands/runtime.rs:102`, `src-tauri/src/child_process.rs:159`, `src-tauri/src/app_settings.rs:6`).

`streaming_cmd` shares generic owned children, bounds stderr/line output and slow consumers, creates exclusive scratch files, and releases registration after cleanup/publication. `ToNewFile` retains overwrite refusal through commit. Canonical source/destination identity and queued-origin validation belong to callers (`src-tauri/src/streaming_cmd.rs:146`, `src-tauri/src/validation.rs:208`, `src-tauri/src/commands/file_transfer.rs:329`).

File-output transfers and SBOM acquire the same process-local destination claim before child/scanner acquisition. A per-job gate orders accepted cancellation, publication and the first terminal outcome; filesystem I/O runs without holding the registry map lock. UI/HTTP cancellation arriving during publication is refused as finishing, rather than reported accepted. Successful publication settles Success before a later cancellation can change it. Exclusive scratch and caller-owned replace/no-replace permission preserve an existing destination when cancellation wins (`src-tauri/src/transfer_registry.rs:128`, `src-tauri/src/transfer_registry.rs:259`, `src-tauri/src/transfer_registry.rs:445`, `src-tauri/src/transfer_registry.rs:223`, `src-tauri/src/streaming_cmd.rs:441`, `src-tauri/src/commands/file_transfer.rs:40`, `src-tauri/src/commands/security_scan.rs:615`).

`host_features` guards VM/Kubernetes/runtime-install commands, service adapters, raw HTTP dispatch and generic tool construction. Windows capability detection uses native CLI presence and the captured connection separately; unsupported features have explicit metadata. Connection commands expose active/candidate/pending state without retargeting and perform shutdown admission/cleanup before requesting explicit restart (`src-tauri/src/host_features.rs:5`, `src-tauri/src/helpers.rs:107`, `src-tauri/src/commands/system_capabilities.rs:173`, `src-tauri/src/commands/engine_connection.rs:16`, `src-tauri/src/commands/engine_connection.rs:72`). Safe-only command SSE reuses bounded sandbox execution; raw event/self-heal work carries original generation. Read `docs/ai.md` for authority checks. Source/compiler checks do not qualify the complete Windows application.

ConPTY keeps duplicate parent pipe ends in an owner declared after Master, so later acquisition errors close those ends before Master joins its output pump. Normal creation releases them at the existing success point. The native failed-launch parent/child guard authors three actual bad-executable failures under bounded capture, plus the existing normal-console control; no native execution is claimed (`src-tauri/src/windows_pty.rs:193`, `src-tauri/src/windows_pty.rs:542`).

### docker_state.rs
- Uses verified captured Windows connection or existing POSIX socket discovery (`src-tauri/src/docker_state.rs:33`)
- Owns one daemon event stream; SSE subscribes to raw events (`src-tauri/src/docker_state.rs:108`, `src-tauri/src/sse.rs:151`)
- Shares raw container/image snapshot reads across native/SSE projections; unavailable or partial reads remain errors (`src-tauri/src/docker_state.rs:466`)
- Rechecks the original Windows identity on reconnect and fences stale reads across availability generations (`src-tauri/src/docker_state.rs:424`)

### instance_reader.rs (226 lines)
- POSIX reads Colima instance configs from `~/.colima/` YAML files; Windows does not start VM polling/publishing or expose VM tray actions (`src-tauri/src/poller.rs:32`, `src-tauri/src/sse.rs:264`, `src-tauri/src/tray.rs:241`)
- Returns instance list in <1ms (vs 30-60s via `colima list` CLI)
- Parses status, CPU, memory, disk, runtime, arch, K8s config

### terminal_session.rs and windows_pty.rs
- One IPC Channel lifecycle supports `ContainerExec` alongside existing POSIX Colima/Lima/Kubernetes targets; Windows accepts Docker containers only (`src-tauri/src/commands/terminal.rs:22`, `src-tauri/src/terminal_session.rs:106`, `src-tauri/src/terminal_session.rs:285`)
- Preflight and creation revalidate stable container/original-engine/running/nonpaused state; no HTTP terminal or host fallback (`src-tauri/src/terminal_session.rs:643`)
- Central cap 16, bounded UTF-8 output, nonzero resize, origin input fences, exit release and closing-time acquisition refusal (`src-tauri/src/terminal_session.rs:52`, `src-tauri/src/terminal_session.rs:87`, `src-tauri/src/terminal_session.rs:486`, `src-tauri/src/terminal_session.rs:570`, `src-tauri/src/terminal_session.rs:608`)
- Native ConPTY atomically includes the child in its Job and drains bounded I/O through Close (`src-tauri/src/windows_pty.rs:30`, `src-tauri/src/windows_pty.rs:156`)
- App exit waits for owned transfer/process settlement; timeout prevents exit and emits `shutdown-blocked` (`src-tauri/src/lib.rs:404`, `src-tauri/src/commands/file_transfer.rs:839`)

### Compose locations

The existing `project_dir` field accepts a client file (`-f`) or directory (`--project-directory`). Windows project actions resolve recorded client-side `ConfigFiles`; malformed project lists/log failures remain errors. POSIX project actions retain launch-directory behavior (`src-tauri/src/commands/compose.rs:18`, `src-tauri/src/commands/compose.rs:157`, `src-tauri/src/commands/compose.rs:211`, `src-tauri/src/commands/compose.rs:238`).
