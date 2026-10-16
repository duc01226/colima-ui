# Frontend Reference

The frontend is built with **Svelte 5** (runes-based reactivity), **TypeScript**, and **Vite 8**. It supports both native desktop (Tauri) and browser modes.

## Directory Structure

```
src/
├── App.svelte                  # Main shell: sidebar, page routing, event listeners
├── main.ts                     # Svelte mount entry point
├── index.css                   # Global styles import
├── vite-env.d.ts               # Vite type declarations
├── setupTests.ts               # Test setup (jsdom)
├── store.svelte.ts             # Global reactive state
├── pages/                      # 19 page components
├── components/                 # 43 components (incl. activity/, security/, topology/, notifications/, settings/, transfer/, diagnostics/)
├── store/                      # Domain-specific state modules
├── lib/                        # Utilities, API layer, AI bus
├── styles/                     # CSS design system
└── locales/                    # i18n translations (en, ja, vi, zh)
```

## Pages

| Page | File | Description |
|------|------|-------------|
| Dashboard | `Dashboard.svelte` | System overview, resource counts, quick navigation |
| Instances | `Instances.svelte` | Colima instance & Kind cluster management |
| Containers | `Containers.svelte` | Docker container list, actions, stats, logs |
| Images | `Images.svelte` | Docker image management |
| Volumes | `Volumes.svelte` | Docker volume management |
| Networks | `Networks.svelte` | Docker network management |
| Activity | `Activity.svelte` | Live container metrics; holds the SSE subscription that gates backend sampling |
| Topology | `Topology.svelte` | Docker graph: containers ↔ networks/volumes/projects/images. Distinct from the Kubernetes graph in `XRay.svelte` |
| Compose | `Compose.svelte` | Docker Compose project management |
| Kubernetes | `Kubernetes.svelte` | Multi-resource K8s browser |
| Linux VMs | `LinuxVMs.svelte` | Lima VM management |
| Models | `Models.svelte` | Ollama AI model management |
| Terminal | `Terminal.svelte` | Retained native xterm.js workspace with Docker/VM/Kubernetes target picker |
| Terminal Instance | `TerminalInstance.svelte` | Individual terminal tab |
| Settings | `Settings.svelte` | System info, disk usage, prune |
| X-Ray | `XRay.svelte` | Kubernetes diagnostics |
| Cluster Topology | `ClusterTopology.svelte` | Cluster visualization |

## Components

| Component | Purpose |
|-----------|---------|
| `AiChatPanel.svelte` | AI diagnostic agent chat panel with tool execution and feedback |
| `ConfirmDialog.svelte` | Reusable confirmation dialog (destructive actions) |
| `ContextMenu.svelte` | Right-click context menus |
| `ErrorBoundary.svelte` | Error boundary wrapper |
| `SetupWizard.svelte` | First-run guided setup; Windows reuses the existing-engine connection form |
| `GettingStartedTour.svelte` | Interactive feature walkthrough with host-available destinations/copy |
| `Icon.svelte` | Dynamic icon component |
| `Icons.svelte` | SVG icon definitions |
| `KubernetesHealth.svelte` | Cluster health dashboard |

### Windows connection and host availability

Setup and Settings reuse `EngineConnectionSettings` with the existing native select/buttons and SettingsSection layout. Platform detection precedes setup; Windows offers existing-engine selection and language, while platform failure stays retryable. The active engine, draft/checked candidate and saved next launch are separate. Continue requires the current active engine to be ready; checking another choice does not connect it. Persistence failure retains the draft, and Save never restarts implicitly (`src/components/SetupWizard.svelte:298`, `src/pages/Settings.svelte:49`, `src/components/settings/EngineConnectionSettings.svelte:25`, `src/lib/engineConnection.svelte.ts:77`).

An explicit restart dialog discloses terminal closure, checks active transfers and offers the notification panel for finishing/cancelling them; Cancel restores focus and preserves the current session. The backend repeats admission and cleanup before requesting restart. App/sidebar show active-engine status and recovery; ordinary Windows views wait for known host policy, readiness, matching C/I origin and a successful complete resource read for that generation. Terminal remains retained outside ordinary view admission (`src/components/settings/EngineConnectionSettings.svelte:34`, `src/components/settings/EngineConnectionSettings.svelte:48`, `src-tauri/src/commands/engine_connection.rs:72`, `src/App.svelte:80`, `src/App.svelte:202`, `src/App.svelte:284`).

Ordinary Windows resource pages unmount while unavailable. App holds memory-only recovery for one active page and original context/endpoint/engine, with generation as revision. It restores choices only after the current complete membership is available, so a partial event cannot consume saved IDs against cleared lists. Registered fields and neutral selection/filter/tab choices restore once; navigation, origin change and App disposal discard them. Resource-specific choices bind to fresh membership; private typed paths/environment stay out of settings and notifications. No old object, output, callback, approval or busy state is restored (`src/App.svelte:86`, `src/lib/recoveryDrafts.svelte.ts:17`, `src/pages/Containers.svelte:504`, `src/components/transfer/TransferDialog.svelte:50`).

Containers/Images/Networks/Volumes retain checked identities and request fresh detail output; later search/filter edits clear checked rows. Topology restores filters and a node only after a fresh graph; Activity restores sorting/tab/history-kind and a container only after a fresh sample. Compose matches a fresh project name and selected tab without diagnosis replay. Security matches both image content ID and reference, retains tab/strictness/severity/passed preferences, and requires a new explicit scan. Activity and Security own nested preferences through bound child props (`src/pages/Images.svelte:34`, `src/pages/Topology.svelte:49`, `src/pages/Activity.svelte:55`, `src/pages/Compose.svelte:26`, `src/pages/Security.svelte:67`, `src/pages/Activity.svelte:288`, `src/pages/Security.svelte:449`).

Image pulling retains pending/rejected user input. Only a successful submission with the same edit revision clears it, including the edit-away-and-back case. Expected form rejection is reported locally; automation overrides do not own or clear the user draft and still report failure to their caller (`src/pages/Images.svelte:92`).

Capability helpers own navigation/empty-state wording; missing CLI, stopped engine, unsupported features and healthy empty resources remain distinct. Windows hides VM/Kubernetes/model-serving controls, adjusts tour copy and checks the Compose plugin separately. Docker terminal targets remain independent of VM/Kubernetes targets. Connection/recovery copy is present in en/vi/zh/ja; native assistive reading, focus/reflow and localized runtime flows remain unqualified (`src/store/capabilities.svelte.ts:149`, `src/components/Sidebar.svelte:103`, `src/components/GettingStartedTour.svelte:108`, `src/pages/Compose.svelte:30`, `src/pages/Terminal.svelte:93`, `src/locales/en.json:578`, `src/locales/vi.json:578`, `src/locales/zh.json:578`, `src/locales/ja.json:578`).

## State Management

Uses **Svelte 5 Runes** (`$state`, `$derived`, `$effect`) for fine-grained reactivity.

### Global Store (`store.svelte.ts`)

```typescript
// Reactive state using Svelte 5 runes
let instances = $state<ColimaInstance[]>([]);
let containers = $state<DockerContainer[]>([]);
let images = $state<DockerImage[]>([]);
// ... volumes, networks, compose, system info
```

### Domain Stores (`store/`)

| Store | Contents |
|-------|----------|
| `ai.svelte.ts` | Chat messages, provider config, model selection, agent state |
| `k8s.svelte.ts` | Namespaces, pods, deployments, services, selected context |
| `confirm.svelte.ts` | Confirmation dialog visibility and callbacks |

## API Layer (`lib/api.ts`)

The dual-mode API layer automatically routes calls based on runtime:

```typescript
import { isRunningInTauri } from './env';   // never inline the check
const IS_TAURI = isRunningInTauri();

// Desktop: invoke Tauri command
// Browser: HTTP fetch to localhost:11420
async function listContainers(all: boolean) {
  if (IS_TAURI) {
    return invoke('list_containers', { all });
  } else {
    return apiFetch(`/api/containers?all=${all}`);
  }
}
```

### API Sections

| Namespace | Methods |
|-----------|---------|
| `instanceApi` | list, start, stop, delete, status, ssh |
| `dockerApi` | containers, images, volumes, networks, compose |
| `k8sApi` | pods, deployments, services, configmaps, etc. |
| `limaApi` | list, start, stop, delete, shell, templates |
| `modelApi` | list, pull, serve, delete |
| `aiApi` | chat, listModels, history, feedback |
| `sysMethods` | checkSystem, hostSpecs, platform, tools |
| `settingsApi` | get/set settings, presets |
| `knowledgeBankApi` | query, learn, feedback, memory |
| `sandboxApi` | classify, execute, executeApproved |

### Interactive Docker terminal

Containers exposes Open terminal in row overflow and the Exec detail pane; `containerTerminalTarget` obtains a backend-verified stable ID and engine before handing a typed `containerExec` target to the existing retained Terminal view. The Docker picker loads independently of VM/Kubernetes availability and uses the common New Session count (`src/pages/Containers.svelte:45`, `src/pages/Containers.svelte:76`, `src/lib/terminal-transport.ts:82`, `src/pages/Terminal.svelte:66`, `src/pages/Terminal.svelte:176`).

Terminal sessions use IPC Channel output and create/write/resize/poll-exit/close commands; browser mode has no HTTP terminal fallback. `TerminalInstance` closes late acquisitions after unmount, disables input after end/error and preserves the named container/engine for Logs recovery. Native buttons and a modal picker own session controls; xterm owns Escape/editor keys, with a visible Focus terminal button and Ctrl+Shift+F6 focus exit (`src/lib/terminal-transport.ts:96`, `src/pages/Terminal.svelte:253`, `src/pages/Terminal.svelte:309`, `src/pages/TerminalInstance.svelte:107`, `src/pages/TerminalInstance.svelte:130`, `src/pages/TerminalInstance.svelte:191`, `src/pages/TerminalInstance.svelte:209`, `src/lib/keyboardShortcuts.ts:25`). Authored lifecycle tests and screenReaderMode are preparation; native accessibility and Windows behavior remain unqualified.

## Event System

### Desktop Mode (Tauri Events)

App owns `startDataPoller` and cleanup. The shared resource refetch fences fulfillment and rejection by run/revision/ready Windows generation; all four resource-page refreshes and the retained Terminal picker use it on Windows. Native POSIX and browser full refreshes publish successful C/I, volume and network groups independently; a failed peer retains prior membership and the shared bounded retry remains lifetime-owned. Their delayed volume/network reads publish independently too. For already-mounted volume/network views, the loss path keeps cleared lists loading through failed delayed reads, even with cached running capabilities. Docker failure does not replace known macOS/Linux VM-management views or their explicit Start controls. Unknown Terminal neither acquires nor presents Docker targets; target preflight failures stay visible alongside shared resource failures. On native Windows, partial container/image updates retain any aggregate read failure; only a complete current four-family refresh clears it. Delayed volume/network acquisition and settlement match both the event generation and complete-refresh outcome revision, with current failures attributed and older results ignored. Late listener acquisition unlistens after disposal, loss clears live resources/detail intent, and `shutdown-blocked` surfaces localized recovery (`src/lib/dataPoller.ts:31`, `src/lib/dataPoller.ts:89`, `src/lib/dataPoller.ts:167`, `src/pages/Terminal.svelte:59`, `src/pages/Terminal.svelte:66`).

Dashboard owns its independent Compose-count outcome: Windows shows an unknown value with loading or failure status until the current read succeeds. Empty success remains zero, the existing card opens Compose for recovery, and other supported platforms retain their cache behavior (`src/pages/Dashboard.svelte:104`, `src/pages/Dashboard.svelte:420`).

### Browser Mode (SSE)

The same poller consumes `docker-state-updated`/`instances-update` through EventSource and owns reconnect/backoff, visible-window fallback polling and teardown. EventSource uses the allowlisted query token because it cannot set headers; native credentials come from IPC and browser credentials from the one-time URL fragment, never an HTTP token endpoint (`src-tauri/src/auth.rs:49`, `src/lib/api/client.ts:249`). Metrics use named-topic SSE in both modes; native API discovery uses the bound-port IPC owner, while browser health discovery is retained. Metrics/transfer discovery uses capped retry delays and cannot open a socket after unsubscribe (`src/lib/dataPoller.ts:274`, `src/lib/api/client.ts:45`, `src/lib/api/metrics.ts:65`, `src/lib/transferEvents.ts:58`).

## Lib Utilities

| Module | Purpose |
|--------|---------|
| `aiEventBus.ts` | Host-filtered AI event advertisement/category/dispatch; backend remains authoritative |
| `aiToolParser.ts` | Parse AI tool-call XML from LLM responses |
| `llmProviders.ts` | Multi-provider LLM integration config |
| `formatters.ts` | Display formatting (bytes, dates, durations) |
| `normalizers.ts` | Data normalization utilities |
| `markdown.ts` | Markdown rendering for AI responses |
| `k8sUtils.ts` | Kubernetes resource utilities |
| `globalToast.ts` | Transient toasts; every toast also becomes a notification entry |
| `transferEvents.ts` | Transfer event subscription, reconnect, and desync signal |
| `transferNotifications.ts` | Wires transfer events into the notification store |
| `osNotify.ts` | Operating-system notifications, desktop only |
| `i18n.svelte.ts` | Internationalization with reactive locale |
| `settingsStore.svelte.ts` | SQLite-backed persistent settings |
| `engineConnection.svelte.ts` | Active snapshot, checked draft and saved-next preference with stale-response fences |
| `presetStateManager.ts` | Instance configuration preset management |

### Notifications and background transfers

Transfers — image export/import and container copies — run in the backend and
outlive the dialog that starts them. The dialog collects paths, starts the job and
**closes**; a multi-gigabyte export used to hold the page behind a modal for its
whole duration.

| Piece | Role |
|-------|------|
| `store/notifications.svelte.ts` | The one list: running jobs, outcomes, and reported errors. Session-scoped, never persisted |
| `components/notifications/` | The centre, opened from the bell in the sidebar header |
| `components/ErrorDetailPanel.svelte` | The same store, filtered to failures |

**Events are the fast path; `transferApi.list()` is the truth.** The SSE channel
drops frames for a client that falls behind and replays nothing after a reconnect,
so every gap — a reconnect, a lag notice, a fresh mount after a reload — triggers a
reconcile against the backend. Without it a transfer that finished while the
connection was down would sit on screen as "running" for the rest of the session,
with a cancel button that does nothing.

Two consequences worth knowing:

- A job the backend no longer lists is marked `ended`, not `success`. Terminal
  entries are retained for about a minute; past that the outcome is genuinely
  unknowable, and reporting a failed export as finished would be worse than saying
  so.
- Entries carry **no host paths** — an image reference, a path inside a container,
  a file name. That is what lets `formatEntryForClipboard` promise its output is
  safe to paste into an issue, and what lets `osNotify` send a title to the
  operating system's own notification centre.

Windows host-path placeholders remain distinct from container paths. Native save-file selection uses Tauri dirname/basename and assigns fields only after both resolve; cancel/path failure preserves typed input. Notification import labels take only the last segment of either slash style; full host paths stay out of that handoff (`src/components/transfer/TransferDialog.svelte:154`, `src/components/transfer/TransferDialog.svelte:80`). Backend confinement remains a separate responsibility; native picker behavior is unqualified.

### Announcements (`lib/announcements.ts`)

The one entry kind whose content comes from outside the machine: a static feed of
release notes and security advisories, fetched by the backend
(`GET /api/announcements`) at start-up and every six hours. Everything the
frontend does with it assumes the feed could be hostile or wrong:

- Text is rendered with normal bindings, never `{@html}`, and is length-capped.
- A `linkUrl` is only offered when it is `https:` on a short host allowlist
  (`isAllowedAnnouncementLink`); otherwise no link is drawn at all. `openExternal`
  refuses non-`https:` URLs for every caller as a second layer.
- `severity: "critical"` bypasses the audience and version filters, so a failed
  entitlement read or an unparseable version bound cannot hide an advisory.
- Ids already shown are remembered in settings, so nothing reappears on restart.
- The channel can be switched off in Settings → Notifications, and off means no
  request is made. See `docs/telemetry.md` for what a request discloses.

### Security page (`pages/Security.svelte`)

Pick a local image, scan it, and see why it scores what it does. Four components
under `components/security/`: the score breakdown, the configuration checklist,
the findings table, and the alternative base images.

- **Nothing is scanned on arrival.** A scan spawns a process, reads a whole
  image and may download a 1.2 GB database first. That is not something to do
  because somebody opened a page.
- **The score is never shown bare.** `ScoreBreakdownCard` prints the scanner,
  its version, the database date and the rule pack version beside the number —
  the same image scores differently against a newer database, and two scanners
  disagree by more than tenfold on identical input.
- **Rule and catalog text is rendered as text.** The rule pack is intended to be
  downloadable later; `{@html}` on downloadable content is how a rule pack turns
  into a script.
- **Alternatives come from a table inside the app** (`resources/catalog/v1.json`),
  not from a query. The image list never leaves the machine.
- The signed update channel for that catalog is not built yet — it needs a
  private key and a publishing process (see the plan's Unresolved #1). Nothing in
  the UI claims otherwise.

### AI Event Bus (`aiEvents/`)

Domain-specific AI tool handlers:

| Module | Tools |
|--------|-------|
| `docker.ts` | Container/image/volume/network operations |
| `colima.ts` | Instance lifecycle, diagnostics |
| `k8s.ts` | Kubernetes resource management |
| `lima.ts` | Lima VM operations |
| `compose.ts` | Docker Compose operations |
| `volumes.ts` | Volume-specific operations |
| `system.ts` | System checks, tool installation |
| `config.ts` | App configuration |

Windows `agentCore` appends the filtered event catalog to native Docker context, gates legacy VM/Kubernetes queries/diagnostics and refuses unavailable events before approval. This changes capability context without additional model calls or authority (`src/lib/agentCore.ts:77`, `src/lib/aiEventBus.ts:28`). Read `docs/ai.md` for the actual sender/backend contracts.

## CSS Design System (`styles/`)

Vanilla CSS with a modular token-based architecture:

| File | Purpose |
|------|---------|
| `tokens.css` | Design tokens (colors, spacing, typography, shadows) |
| `reset.css` | CSS reset / normalize |
| `layout.css` | Grid, flexbox, sidebar layout |
| `components.css` | Buttons, cards, badges, tables |
| `forms.css` | Input, select, checkbox, toggle styles |
| `overlays.css` | Modals, dialogs, tooltips, toasts |
| `features.css` | Feature-specific styles (terminal, AI chat) |

## Testing

```bash
# Unit tests (Vitest + jsdom)
npx vitest run

# Type checking
npm run check    # svelte-check
```

Test files follow the `*.test.ts` convention alongside source files:
- `aiToolParser.test.ts`
- `formatters.test.ts`
- `normalizers.test.ts`
- `markdown.test.ts`
- `i18n.test.ts`
- `src/lib/terminal-transport.test.ts:11` — substituted IPC target/literal input/resize guards
- `src/pages/TerminalInstance.test.ts:38` — held acquisition, late success/failure and running Close; authored tests are not an execution result
