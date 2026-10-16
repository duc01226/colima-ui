# HTTP API Reference

ColimaUI binds its HTTP API to an available loopback port in 11420–11429. Browser UI is the Vite development surface; the packaged router does not serve frontend assets. Public health reports liveness, not engine readiness or credential identity (`src-tauri/src/api_server.rs:366`, `src-tauri/src/api_server.rs:391`).

## Authentication

All endpoints (except public ones) require Bearer token authentication:

```
Authorization: Bearer <token>
```

### Public Endpoints (no auth required)

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/health` | Health check (returns `"ok"`) |

`/api/health` is the only one. There is no endpoint that hands out the token:
the desktop webview gets it over IPC (`auth::api_token`), and a browser gets it
from the debug browser handoff in a URL fragment, consumed once and removed from the address bar. SSE query-token use is explicitly allowlisted; ordinary requests use Bearer auth.

The external `cui` client reads the current nonsecret instance marker through a read-only settings connection, then validates its owner-private `api/<instance>.json` for port/credential. A new instance supersedes the marker before bind and publishes only after successful bind; missing/invalid/unavailable references never trigger a health-port fallback. App exit closes only its own reference after owned work settles. Private-file publication failure preserves existing in-app access (`src-tauri/src/api_reference.rs:72`, `src-tauri/src/api_reference.rs:93`, `src-tauri/src/api_reference.rs:108`, `src-tauri/src/api_reference.rs:125`, `src-tauri/src/api_server.rs:417`, `src-tauri/src/lib.rs:404`). Read [Architecture → HTTP Authentication](architecture.md#http-authentication) when changing these handoffs; never put credential contents in docs or evidence.

Native SSE consumers resolve this instance’s bound port through `api_port` IPC and fail closed while it is unbound; browser development keeps the existing health discovery. No new HTTP port/token endpoint is exposed (`src-tauri/src/api_server.rs:34`, `src/lib/api/client.ts:45`).

Authenticated Windows requests to unsupported VM/Kubernetes/model-serving/runtime-install families return 501 before dispatch; missing credentials still fail authentication first. Installed tools do not broaden host support. Capability metadata reports `unsupported` separately from missing/stopped/unknown, while Docker access and public health retain their contracts (`src-tauri/src/host_features.rs:74`, `src-tauri/src/api_server.rs:366`, `src-tauri/src/commands/system_capabilities.rs:40`).

## SSE Events

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/events` | Server-Sent Events stream (docker state, instances) |

Event types: `docker-state`, `instances`, `docker-event`

---

## System

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/system/check` | System info (colima, docker, lima versions) |
| `GET` | `/api/system/version` | Colima version string |
| `GET` | `/api/system/homebrew` | Homebrew status |
| `GET` | `/api/system/check-tool` | Check tool installation (`?name=kubectl`) |
| `GET` | `/api/system/platform` | OS/arch/package manager info |
| `GET` | `/api/system/host-specs` | Host CPU/memory specs |
| `POST` | `/api/system/install` | Install dependency (`{name, method}`) |
| `POST` | `/api/system/prune` | Docker system prune (`?all=true`) |
| `GET` | `/api/system/df` | Docker disk usage |
| `GET` | `/api/settings` | Get all settings |
| `POST` | `/api/settings` | Set a setting (`{key, value}`) |

---

### Native connection preferences

The desktop-only `connectionApi` uses `get_engine_connection`, `check_engine_connection`, `save_engine_connection` and `restart_for_engine`; these are IPC commands, not HTTP connection routes. Reads separate active status, saved pending choice, context choices and discovery errors. Candidate checking does not make it active; saving validates the choice and changes only next launch. The generic `docker.context` settings writer applies the same validation (`src/lib/api/system.ts:197`, `src-tauri/src/commands/engine_connection.rs:16`, `src-tauri/src/commands/knowledge_bank.rs:961`).

Explicit restart first admits shutdown under the transfer registry lock. With `cancelTransfers=false`, active work refuses restart without closing admission; finish or explicitly cancel it and retry. After admission, terminal closure and transfer/stream/child settlement must succeed before `request_restart`. Native restart outcome remains unqualified (`src-tauri/src/commands/engine_connection.rs:72`, `src-tauri/src/transfer_registry.rs:184`).

---

## Colima Instances

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/instances` | List all Colima instances |
| `POST` | `/api/instances/start` | Start instance (JSON: `StartConfig`) |
| `POST` | `/api/instances/stop` | Stop instance (`?profile=default&force=false`) |
| `POST` | `/api/instances/delete` | Delete instance (`?profile=default&force=false`) |
| `GET` | `/api/instances/status` | Instance status (`?profile=default`) |
| `GET` | `/api/instances/ssh` | Get SSH command (`?profile=default`) |
| `POST` | `/api/instances/k8s` | Kubernetes action (`?profile=default&action=start`) |

---

## Docker Containers

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/containers` | List containers (`?all=false`) |
| `POST` | `/api/containers/start` | Start container (`?containerId=...`) |
| `POST` | `/api/containers/stop` | Stop container (`?containerId=...`) |
| `POST` | `/api/containers/restart` | Restart container (`?containerId=...`) |
| `POST` | `/api/containers/remove` | Remove container (`?containerId=...&force=false`) |
| `GET` | `/api/containers/logs` | Container logs (`?containerId=...&lines=100`) |
| `GET` | `/api/containers/inspect` | Inspect container (`?containerId=...`) |
| `GET` | `/api/containers/stats` | Container stats (`?containerId=...`) |
| `GET` | `/api/containers/stats/all` | All container stats |
| `GET` | `/api/containers/top` | Container processes (`?containerId=...`) |
| `POST` | `/api/containers/exec` | Exec in container (`{containerId, command}`) |
| `POST` | `/api/containers/run` | Run new container (`{image, name, ports, env, ...}`) |
| `POST` | `/api/containers/rename` | Rename container (`{containerId, newName}`) |
| `POST` | `/api/containers/pause` | Pause container (`?containerId=...`) |
| `POST` | `/api/containers/unpause` | Unpause container (`?containerId=...`) |

---

## Docker Images

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/images` | List images |
| `POST` | `/api/images/remove` | Remove image (`?imageId=...&force=false`) |
| `POST` | `/api/images/pull` | Pull image (`?imageName=...`) |
| `POST` | `/api/images/prune` | Prune unused images |
| `GET` | `/api/images/inspect` | Inspect image (`?imageId=...`) |
| `POST` | `/api/images/tag` | Tag image (`{imageId, newTag}`) |

---

## Docker Volumes

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/volumes` | List volumes |
| `POST` | `/api/volumes/create` | Create volume (`{name, driver}`) |
| `POST` | `/api/volumes/remove` | Remove volume (`?name=...&force=false`) |
| `POST` | `/api/volumes/prune` | Prune unused volumes |
| `GET` | `/api/volumes/inspect` | Inspect volume (`?name=...`) |

---

## Docker Networks

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/networks` | List networks |
| `POST` | `/api/networks/create` | Create network (`{name, driver, subnet}`) |
| `POST` | `/api/networks/remove` | Remove network (`?name=...`) |
| `GET` | `/api/networks/inspect` | Inspect network (`?name=...`) |
| `POST` | `/api/networks/prune` | Prune unused networks |

---

## Diagnostics

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/diagnostics/bundle` | Build a redacted bundle (`{error?, containerId?, logLines?}`) |
| `POST` | `/api/diagnostics/save` | Write selected sections as Markdown (`{bundle, include[], destDir, fileName, overwrite?}`) |

Returns `{sections[], signature, appVersion, truncatedBytes}`. **Nothing is
transmitted** — there is no send path in the backend; the response goes to the
caller and sharing is the user's action.

Every section is redacted at construction, so a section that skipped redaction
cannot exist. `save` redacts the rendered Markdown again, because the bundle
round-trips through the client before being written. Container environment
variables are never collected; container logs are, opt-in and off by default.

`signature` uses the same algorithm as the Knowledge Bank's error matching, so the
same failure on two machines produces the same string and duplicate reports can be
grouped.

Logs are capped at 5000 lines and 2 MB *before* redaction — `redact` costs about
0.8 s/MB, so bounding the input is a latency budget as much as a byte budget.

---

## Live Metrics

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/metrics/status` | Read existing collector/engine state without activating monitoring |
| `POST` | `/api/metrics/interval` | Sampling period in ms (`{ms}`); clamped to 1000–60000, returns what was applied |

`GET /api/metrics/status` is an authenticated, read-only observer of existing collector/engine state. It does not initialize the engine, subscribe, start a tick or change the interval. It requires the normal Authorization header and adds no query-token exception (`src-tauri/src/api_server.rs:170`, `src-tauri/src/commands/metrics_collector.rs:94`, `src-tauri/src/auth.rs:49`).

Samples are not fetched — they arrive on the SSE stream. Subscribe with
`GET /api/events?topics=metrics.sample`; each event carries
`{samples[], engine, intervalMs}`.

**Naming the topic is what starts sampling.** The collector runs only while
`subscriber_count("metrics.sample")` is above zero, and that count is tied to the
lifetime of the HTTP stream — a closed tab, a reload or a crashed client all stop
it with no cooperation. A client that subscribes without naming the topic still
receives every event but is counted as watching nothing, so it will not keep the
sampler alive on its own.

A `stream-lagged` event means samples were dropped for that client. Render a gap;
interpolating across it asserts data nobody observed.

---

## Background Transfers

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/images/save` | Export images to a TAR (`{images[], destDir, fileName, overwrite?}`) |
| `POST` | `/api/images/load` | Import a TAR (`{tarPath}`) |
| `POST` | `/api/containers/cp/to` | Copy a host file into a container (`{containerId, hostPath, containerPath}`) |
| `POST` | `/api/containers/cp/from` | Copy a container path out **as a TAR archive** (`{containerId, containerPath, destDir, fileName, overwrite?}`) |
| `POST` | `/api/transfers/cancel` | Cancel a running transfer (`{jobId}`) |
| `GET` | `/api/transfers` | List transfers this process knows about |

These return `{jobId, totalEstimate}` immediately and run in the background.
Progress arrives as `transfer.progress` events, ending in `transfer.done`
(which carries `cancelled`) or `transfer.failed`. `totalEstimate` comes from image
metadata and is an estimate, not a total — Docker reports none for these
operations.

## Security

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/security/scan` | Scan one local image for known vulnerabilities |
| `POST` | `/api/security/scan/cancel` | Stop a running scan by `scanId` |
| `POST` | `/api/security/sbom` | Write an SBOM (CycloneDX or SPDX-JSON) to a file |
| `POST` | `/api/security/audit` | Scan + configuration rules + score, in one call |
| `GET` | `/api/security/rules` | The rule pack this build carries |
| `GET` | `/api/security/alternatives?image=…` | Base images worth considering instead |

`/api/security/alternatives` answers from a table compiled into the app. It is a
lookup, not a query to a service: asking a server "what should I use instead of
X?" would send the user's image list to us, which says what they run and
sometimes what they are building before it is public.

`/api/security/audit` is one endpoint rather than three because a score is only
meaningful beside the findings and rule results it came from; assembling it
client-side is how a score ends up displayed next to a different image's scan.

The score is four numbers plus a total: **vulnerabilities /40** (from the scan),
**hardening /25**, **provenance /20**, **freshness /15** (from the rules). The
total exists to sort a list — the breakdown is the answer, because "42/100" says
nothing about whether the problem is CVEs or a root user on a moving tag.

Every score carries `inputs` — rule pack version, scanner, scanner version,
database date, strictness level. Show them next to the number: the same image
scores differently against a newer database, and two scanners disagree by more
than 10× on identical input. `level` is `l1` (default), `l2` or `l3`; higher
levels enable more rules and can therefore only lower a score.

The scanner is **Trivy on the host**, not bundled — its vulnerability database
alone is ~1.2 GB and changes daily. Whether it is installed comes from
`/api/capabilities` under the id `trivy`; a missing scanner is a capability
state with an install hint, not an API error.

`scanId` is chosen by the caller so a scan can be cancelled before its process
exists. Progress arrives on the SSE stream as `security-scan-progress` with a
`stage` of `database` or `scan` — the first scan in a while spends most of its
time on the database, and reporting that as scanning would look like a hang.

Results are cached by **image digest**, never by tag: rebuilding `app:latest`
and scanning again must not return the previous image's findings. Pass
`refresh: true` to bypass it.

File-output transfers and SBOM acquire the same process-local destination claim before child/scanner acquisition. A per-job gate orders accepted cancellation, publication and the first terminal outcome; filesystem I/O runs without holding the registry map lock. UI/HTTP cancellation arriving during publication is refused as finishing, rather than reported accepted. Successful publication settles Success before a later cancellation can change it. Exclusive scratch and caller-owned replace/no-replace permission preserve an existing destination when cancellation wins (`src-tauri/src/transfer_registry.rs:128`, `src-tauri/src/transfer_registry.rs:259`, `src-tauri/src/transfer_registry.rs:445`, `src-tauri/src/transfer_registry.rs:223`, `src-tauri/src/streaming_cmd.rs:441`, `src-tauri/src/commands/file_transfer.rs:40`, `src-tauri/src/commands/security_scan.rs:615`).

Nothing is pulled. An image that is not present locally is an error saying so,
because pulling spends bandwidth the user did not agree to spend. Scanning reads
the image through the local runtime; image names and results stay on the machine.

## Announcements

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/announcements` | Vendor announcements: releases, advisories, maintenance |

A one-way, read-only channel. The backend fetches one hard-coded static JSON file
and returns it verbatim; **nothing about the user is sent** — no id, no cookie, no
custom header, and the request takes no parameters.

Fetching happens in the backend rather than the webview so the app's CSP does not
have to list an external host, and so exactly one URL is reachable instead of the
`https://*` the HTTP plugin's capability would allow.

A feed that cannot be read is an **error**, never an empty feed: the client keeps
showing what it already had rather than clearing the list on a flaky network. A feed
declaring a `version` newer than the build understands is refused rather than
guessed at; unknown *fields* are ignored, so a feed written for a later build does
not break an earlier one.

Note that fetching at all reveals an IP, a timestamp and the app version to the
host. See `docs/telemetry.md`; the channel can be switched off, and switching it off
stops the request rather than filtering its result.

### Reconciling, not just listening

Events are the fast path and are **lossy**: the SSE channel drops frames under lag
by design, and a job can reach a terminal state before the caller even holds its id
(the command is spawned before `start` returns). `GET /api/transfers` is the source
of truth — call it on connect and after every reconnect.

Each entry is
`{jobId, kind, status, bytes, totalEstimate, startedAt, finishedAt, targetLabel, error}`,
where `status` is `starting | running | success | failed | cancelled` and `kind` is
`save | load | cp-in | cp-out | sbom`. Finished transfers stay listed for about 60 seconds,
which is what lets a client that missed the terminal event still learn the outcome;
after that, absence no longer establishes the outcome. File-output `status: "success"` follows destination publication; load/copy-in jobs instead require successful runtime completion. `targetLabel` is an image reference or a path inside a container — never a
host path, so this payload needs no redaction. `error` is redacted at the source.

`POST /api/transfers/cancel` returns `cancelled | alreadyFinished | unknownJob` for admitted/settled/absent jobs. If publication currently owns the job gate, cancellation returns an error asking the caller to wait; it must not be treated as accepted. A `cancelled` acknowledgement records intent before kill, and late worker acquisition rechecks settlement and disarms stale pending cancellation (`src-tauri/src/transfer_registry.rs:445`, `src-tauri/src/commands/file_transfer.rs:877`, `src-tauri/src/commands/file_transfer.rs:885`). Nothing is persisted. Quit waits for consumer/stream/child settlement; cleanup timeout prevents exit.

A transfer is refused at start if another unfinished transfer is already writing to
the same destination. The existence check alone cannot see this, because the first
job's output is not at that path until it completes.

Transfer outcomes are monotonic: cancelled/terminal jobs cannot become Running again, and the first terminal settlement is retained. Shutdown admission and registration share a lock, preventing new transfers after admitted cleanup (`src-tauri/src/transfer_registry.rs:194`, `src-tauri/src/transfer_registry.rs:216`, `src-tauri/src/transfer_registry.rs:184`).

### Archive contract

`fileName` must end in `.tar` for `/api/images/save` and `/api/containers/cp/from`.
Both write an **uncompressed** archive, so `.tar.gz`, `.tgz` and `.zip` are refused
rather than honoured under a name that would misdescribe the contents.
`/api/images/load` rejects a file that is not a TAR before the runtime is started.

`/api/containers/cp/from` returns an archive **whether the container path is a file
or a directory**. It previously wrote the path directly, which produced a directory
for a directory source — progress then measured a directory entry rather than data,
and cancelling could not remove it. Callers that expected a bare file must extract:
`tar -xf <file>`.

Output uses an exclusively created `<fileName>.<job-id>.part` sibling and publishes only after command success. Replace permission stays with the caller through commit; without overwrite, a destination appearing during streaming is refused rather than replaced. Cancellation/failure cleans owned scratch and preserves the prior destination (`src-tauri/src/streaming_cmd.rs:400`, `src-tauri/src/streaming_cmd.rs:441`, `src-tauri/src/streaming_cmd.rs:476`).

`destDir` and `fileName` are separate fields because the write is confined to the
folder the caller named: checking a joined path against its own parent would pass
by construction. A `fileName` containing `../` is refused before any command runs.

A file picker in the desktop UI records the user's intent; it is not an
authorization mechanism, and these routes are reachable with a valid token
regardless. Confinement is enforced in `commands::file_transfer` on every path.

---

## Docker Topology

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/topology` | Docker graph for the current engine |

Returns `{nodes, edges, warnings}`. Nodes are `container`, `network`, `volume`,
`project` and `image`, with ids namespaced by kind (`container:abc`). Edges always
start at a container. `warnings` is non-empty when a subsystem could not be
listed — the graph is then incomplete rather than empty, and the caller should say
so instead of implying those attachments do not exist.

Takes no `instance` parameter: nothing in the app targets a second engine.

---

## Docker Compose

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/compose` | List compose projects |
| `POST` | `/api/compose/up` | Compose up (`{projectDir, detach}`); client file or directory |
| `POST` | `/api/compose/down` | Compose down (`{projectName}`) |
| `POST` | `/api/compose/restart` | Compose restart (`{projectName}`) |
| `GET` | `/api/compose/logs` | Compose logs (`?projectName=...&lines=100`) |
| `GET` | `/api/compose/ps` | Compose ps (`?projectName=...`) |

---

The existing `projectDir` field means a client-side file (`-f`) or directory (`--project-directory`), not a daemon mount path. Windows named-project actions resolve the listed client `ConfigFiles` and refuse missing/malformed metadata; POSIX project actions retain launch-directory behavior. List/log failures remain errors (`src-tauri/src/commands/compose.rs:18`, `src-tauri/src/commands/compose.rs:157`, `src-tauri/src/commands/compose.rs:211`, `src-tauri/src/commands/compose.rs:238`).

## Docker System

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/docker/df` | Docker disk usage |
| `POST` | `/api/docker/prune` | Full system prune (`?confirm=true` required) |

---

## Kubernetes

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/k8s/check` | Check kubectl connectivity |
| `GET` | `/api/k8s/namespaces` | List namespaces |
| `GET` | `/api/k8s/pods` | List pods (`?namespace=default`) |
| `GET` | `/api/k8s/services` | List services (`?namespace=default`) |
| `GET` | `/api/k8s/deployments` | List deployments (`?namespace=default`) |
| `GET` | `/api/k8s/nodes` | List nodes (text) |
| `GET` | `/api/k8s/nodes/json` | List nodes (JSON) |
| `GET` | `/api/k8s/events` | List events (text, `?namespace=default`) |
| `GET` | `/api/k8s/events/json` | List events (JSON) |
| `GET` | `/api/k8s/resources` | List resources by kind (`?kind=...&namespace=...`) |
| `GET` | `/api/k8s/pods/logs` | Pod logs (`?name=...&namespace=...&lines=100`) |
| `GET` | `/api/k8s/pods/logs/stream` | Stream pod logs (SSE) |
| `GET` | `/api/k8s/pods/containers` | List pod containers |
| `GET` | `/api/k8s/pods/container-logs` | Container-specific logs |
| `POST` | `/api/k8s/pods/delete` | Delete pod |
| `GET` | `/api/k8s/describe` | Describe resource |
| `GET` | `/api/k8s/resources/yaml` | Get resource YAML |
| `POST` | `/api/k8s/scale` | Scale deployment |
| `POST` | `/api/k8s/scale-generic` | Scale any scalable resource |
| `POST` | `/api/k8s/resources/delete` | Delete resource |
| `POST` | `/api/k8s/resources/restart` | Restart resource |
| `POST` | `/api/k8s/apply` | Apply YAML |
| `POST` | `/api/k8s/exec` | Exec into pod |
| `POST` | `/api/k8s/nodes/action` | Node action (cordon/drain/uncordon) |
| `GET` | `/api/k8s/contexts` | List kubeconfig contexts |
| `GET` | `/api/k8s/contexts/current` | Current context |
| `POST` | `/api/k8s/contexts/set` | Switch context |
| `GET` | `/api/k8s/crds` | List CRDs |
| `GET` | `/api/k8s/crds/resources` | List CRD resources |
| `GET` | `/api/k8s/cluster-health` | Cluster health scan |
| `POST` | `/api/k8s/benchmark` | HTTP benchmark |

### Port Forwarding

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/k8s/port-forward/start` | Start port forward |
| `POST` | `/api/k8s/port-forward/stop` | Stop port forward |
| `GET` | `/api/k8s/port-forward/list` | List active forwards |

### Kind Clusters

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/kind` | List Kind clusters |
| `POST` | `/api/kind/create` | Create Kind cluster |
| `POST` | `/api/kind/delete` | Delete Kind cluster |

---

## Lima VMs

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/lima` | List Lima VMs |
| `POST` | `/api/lima/start` | Start VM (`{name}`) |
| `POST` | `/api/lima/stop` | Stop VM (`{name}`) |
| `POST` | `/api/lima/delete` | Delete VM (`{name, force}`) |
| `GET` | `/api/lima/info` | Lima system info |
| `POST` | `/api/lima/shell` | Execute command in VM (`{name, command}`) |
| `GET` | `/api/lima/templates` | List available templates |
| `POST` | `/api/lima/create` | Create VM (`{name, template, cpus, memory, disk}`) |

---

## AI Models (Ollama)

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/models` | List models (`?profile=...`) |
| `POST` | `/api/models/pull` | Pull model (`?profile=...&modelName=...`) |
| `POST` | `/api/models/serve` | Serve model (`?profile=...&modelName=...&port=...`) |
| `POST` | `/api/models/delete` | Delete model (`?profile=...&modelName=...`) |

---

## AI Chat & Diagnostics

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/ai/chat` | AI chat message |
| `POST` | `/api/ai/models` | List AI provider models |
| `POST` | `/api/ai/search` | SearXNG web search |
| `POST` | `/api/ai/fetch-page` | Fetch page as markdown |
| `GET` | `/api/ai/context` | Get app context for AI |
| `POST` | `/api/cli/chat` | Headless CLI chat (for external agents) |

---

## Knowledge Bank

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/kb/query` | Query knowledge bank |
| `POST` | `/api/kb/search` | Search memories |
| `GET` | `/api/kb/memories` | Get all memories |
| `POST` | `/api/kb/memories/update` | Update a memory |
| `POST` | `/api/kb/memories/delete` | Delete a memory |

---

## Shell Sandbox

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/sandbox/execute` | Execute safe command |
| `POST` | `/api/sandbox/execute-approved` | Execute user-approved command |
| `POST` | `/api/cli/execute_stream` | Safe-only command; bounded completed output over SSE |

---

## Diagnostics

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/diagnostics/logs` | Collect diagnostic logs |

---

The command SSE route does not carry interactive approval. It invokes the same safe classifier/executor and sends `stdout`, `stderr`, then `exit` after completion, or `error` on refusal/failure. Windows uses captured Docker argv/environment; unsupported commands and target overrides stay denied even through the separately approved endpoint (`src-tauri/src/routes/kb.rs:98`, `src-tauri/src/commands/shell_sandbox.rs:261`, `src-tauri/src/commands/shell_sandbox.rs:289`).

## Terminal Sessions

Terminal sessions are native IPC-only; the HTTP router registers no terminal create/write/read/resize/close route. The frontend uses `terminal_target` preflight, then the existing create/write/resize/poll-exit/close lifecycle with output pushed through an IPC Channel (`src-tauri/src/api_server.rs:351`, `src/lib/terminal-transport.ts:82`, `src/lib/terminal-transport.ts:96`, `src-tauri/src/commands/terminal.rs:22`, `src-tauri/src/lib.rs:353`).

`ContainerExec` carries a stable container ID and original engine; creation revalidates running/nonpaused state and origin. Windows uses ConPTY and permits Docker-container sessions only; no host shell or automatic shell/engine installation is a fallback. Close/exit and origin-loss input fences stay with SessionManager. Browser clients can use ordinary permitted resource/one-command exec routes, but cannot open an interactive terminal (`src-tauri/src/terminal_session.rs:285`, `src-tauri/src/terminal_session.rs:486`, `src-tauri/src/terminal_session.rs:570`, `src-tauri/src/terminal_session.rs:643`, `src-tauri/src/windows_pty.rs:156`). Native Windows qualification remains pending.

---

## Response Format

Ordinary JSON handlers use the response envelope below; authentication failures and SSE/streaming contracts are separate. Terminal IPC has its own results and pushed output.

```json
{
  "success": true,
  "data": "...",
  "error": null
}
```

On error:

```json
{
  "success": false,
  "data": null,
  "error": "Error message"
}
```
