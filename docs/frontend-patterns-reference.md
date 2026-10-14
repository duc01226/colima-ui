# Frontend patterns

> Last scanned: 2026-10-14

Read this before changing Svelte views, stores, API calls, forms or navigation. It records current source patterns; use the cited owner when changing a contract. Source inspection does not establish runtime or accessibility conformance.

## Composition and navigation

The HTML entry mounts one Svelte application. Views use typed `$props`, callback props, rune state and, where two-way ownership is needed, `$bindable`. Content wrappers take `Snippet` children rather than a base-component hierarchy (`src/main.ts:12`, `src/components/RowActions.svelte:27`, `src/components/settings/SettingsSection.svelte:8`, `src/components/settings/SettingsSection.svelte:29`).

`App.svelte` statically imports pages and selects them through `uiState.currentPage`. The active destination is restored from localStorage; this is page state, not a URL router. Ordinary pages unmount when changed. Terminal delays its first mount and then hides the same instance to preserve PTYs; it is not lazy module loading (`src/App.svelte:11`, `src/App.svelte:75`, `src/App.svelte:86`, `src/App.svelte:154`, `src/App.svelte:193`, `src/store.svelte.ts:39`).

Sidebar ordinary items select a page, while AI chat toggles a side panel. Graph/list handoffs set resource intent before navigation; Topology, Containers and Compose currently consume matching focus requests. Preserve the target's resource-identity conversion and read `docs/topology-design-philosophy.md` when changing those flows (`src/components/Sidebar.svelte:393`, `src/lib/topology-link.ts:23`, `src/components/topology/NodeDetail.svelte:84`, `src/pages/Topology.svelte:68`, `src/pages/Containers.svelte:119`, `src/pages/Compose.svelte:38`). Keyboard navigation excludes editable inputs and xterm (`src/lib/keyboardShortcuts.ts:9`).

| Change | Read and reuse the matching owner |
| --- | --- |
| Row/card action layout and overflow | `src/components/RowActions.svelte:6` and `src/components/ContextMenu.svelte:26` — action callbacks stay with the feature; preserve disabled/tone and click isolation. |
| Settings section layout | `src/components/settings/SettingsSection.svelte:8` — title/icon/description, child snippet and optional bound element handle. |
| Transfer input/start | `src/components/transfer/TransferDialog.svelte:2` — four modes share start UI; accepted work moves to global notifications. |
| Confirmation and feedback | `src/components/ConfirmDialog.svelte:2`, `src/components/ToastContainer.svelte:12`, `src/components/notifications/NotificationPanel.svelte:61` — these owners coexist with page-local dialogs. |
| Structured change versus file patch display | `src/components/DiffView.svelte:5` versus `src/components/UnifiedDiffView.svelte:2` — choose by data shape; rendering does not own applying changes. |

## State, requests and events

Module-scoped rune objects own shared Docker/resource/dashboard/navigation data; pages own filters, selection and operation state. `instancesLoaded` distinguishes an unknown cold-start list from loaded-empty. Kubernetes has its own connectivity and list-loading state (`src/store.svelte.ts:3`, `src/store.svelte.ts:19`, `src/store/k8s.svelte.ts:42`, `src/pages/Containers.svelte:24`).

`src/lib/api.ts` is a compatibility re-export; domain wrappers live under `src/lib/api/`. Ordinary `call<T>` chooses native IPC or localhost HTTP, with runtime detection in `env.ts`. HTTP resolves a healthy port from 11420–11429, sends Bearer auth and unwraps the ordinary response envelope. Do not assume every endpoint or raw fetch exception uses that envelope/error conversion (`src/lib/api.ts:1`, `src/lib/api/index.ts:1`, `src/lib/api/client.ts:45`, `src/lib/api/client.ts:103`, `src/lib/api/client.ts:146`, `src/lib/env.ts:1`).

API list mapping and event mapping have separate owners. Docker list wrappers normalize their own results; push updates use `normalizers.ts` through the data poller. Trace both when changing a field (`src/lib/api/docker.ts:7`, `src/lib/normalizers.ts:12`, `src/lib/dataPoller.ts:86`).

The data poller consumes `instances-update` and `docker-state-updated`; browser SSE failure starts visible-window polling and bounded reconnect backoff. Metrics are an explicit exception to dual transport: both runtimes subscribe to SSE because subscriber count controls backend sampling (`src/lib/dataPoller.ts:197`, `src/lib/dataPoller.ts:207`, `src/lib/dataPoller.ts:223`, `src/lib/api/metrics.ts:65`). Historical manual-guide event examples are not the current event contract.

Transfers outlive their dialog. The app owns the session subscription; events provide quick updates, while `transferApi.list()` reconciles authoritative state after connection/gaps. Keep transport adaptation outside the notification store. Unknown old jobs become an explicit ended outcome, not inferred success (`src/App.svelte:103`, `src/components/transfer/TransferDialog.svelte:181`, `src/lib/transferEvents.ts:13`, `src/lib/transferNotifications.ts:62`, `src/store/notifications.svelte.ts:357`).

`reportError` is the intended operation-error presentation owner, with normalization, localization, remediation and session history. Existing owners also use local indicators or direct toasts; this is not proof that every thrown error follows one path (`src/lib/errorReporter.ts:1`, `src/pages/Containers.svelte:191`, `src/pages/Security.svelte:67`). Capability helpers distinguish missing/stopped tools from genuinely empty lists (`src/store/capabilities.svelte.ts:63`).

## Form and lifetime variants

| Owner | Validation, failure and persistence contract |
| --- | --- |
| Network/volume creation | Trimmed-name action checks; submit is disabled while busy or empty. Reset fields on success, retain them after request failure and report via toast (`src/pages/Networks.svelte:64`, `src/pages/Networks.svelte:240`, `src/pages/Volumes.svelte:65`). |
| Colima configuration | Preview on `change`, show field-coded issues after controls, send only changed fields and preserve the loaded mtime contract. Refused apply retains input (`src/pages/settings/ColimaConfig.svelte:123`, `src/pages/settings/ColimaConfig.svelte:161`, `src/pages/settings/ColimaConfig.svelte:181`, `src/pages/settings/ColimaConfig.svelte:355`, `src/pages/settings/ColimaConfig.svelte:426`). |
| Transfer start | Required-field gate and in-place start error retain editability; accepted start hands over the job and closes (`src/components/transfer/TransferDialog.svelte:181`, `src/components/transfer/TransferDialog.svelte:192`, `src/components/transfer/TransferDialog.svelte:202`, `src/components/transfer/TransferDialog.svelte:300`). |
| Model/provider settings | Model free text is deliberate and commits per keystroke; provider-specific keys/endpoints/models have separate ownership. Settings writes update local state first and log persistence failures (`src/components/settings/ModelPicker.svelte:5`, `src/components/settings/ModelPicker.svelte:81`, `src/components/settings/AIPanelSettings.svelte:32`, `src/lib/settingsStore.svelte.ts:47`). |

Page-local drafts do not have a universal navigation/refresh persistence contract. App-mounted AI input survives page changes while mounted; Terminal retains its special lifetime. The AI composer guards Enter-to-send during IME composition (`src/App.svelte:154`, `src/App.svelte:193`, `src/App.svelte:214`, `src/components/AiChatPanel.svelte:133`).

The resource owner must perform its own teardown through returned mount/effect cleanup or `onDestroy`. Guard acquisitions that resolve after unmount. The transfer adapter handles late unlisten acquisition; the data poller's native listeners do not establish that same cleanup guarantee (`src/pages/TerminalInstance.svelte:102`, `src/pages/TerminalInstance.svelte:160`, `src/components/ContextMenu.svelte:66`, `src/lib/transferEvents.ts:65`, `src/lib/dataPoller.ts:145`).

## Recorded interaction conventions

These clause records describe project authority for the cited owners, not a universal implementation or a review verdict. Where no project-wide convention is established, the applicable shared clause/platform baseline still applies; numeric mobile heuristics are not a desktop conformance rule.

| Clause | Project authority or gap, with evidence |
| --- | --- |
| UI-5.2 control states | PROJECT AUTHORITY — overrides UI-5.2: shared button CSS defines default/hover/focus-visible/active/disabled; operation owners disable pending actions and change labels. There is no generic Button loading prop (`src/styles/components.css:20`, `src/styles/components.css:39`, `src/styles/components.css:43`, `src/styles/components.css:55`, `src/styles/components.css:103`, `src/components/transfer/TransferDialog.svelte:311`). |
| UI-7.2 labels/placeholders | PROJECT AUTHORITY — overrides UI-7.2: create/config/transfer forms use visible labels and example placeholders. GAP: searches/composer also use placeholder-driven input; no global visible-label contract was established (`src/pages/Networks.svelte:240`, `src/pages/Networks.svelte:266`, `src/components/transfer/TransferDialog.svelte:228`, `src/components/AiChatPanel.svelte:592`). |
| UI-7.3 timing/error placement | PROJECT AUTHORITY — overrides UI-7.3 is per operation: action-time guards, change-driven previews, in-place transfer errors and toast-only create failures. GAP: no universal timing or field-error association component (`src/pages/Networks.svelte:64`, `src/pages/Networks.svelte:74`, `src/pages/settings/ColimaConfig.svelte:355`, `src/components/transfer/TransferDialog.svelte:300`). |
| UI-7.5 input retention | PROJECT AUTHORITY — overrides UI-7.5: rejected transfer/create operations retain fields. GAP: ordinary page/dialog drafts have no general navigation or refresh preservation; destination persistence is separate (`src/components/transfer/TransferDialog.svelte:192`, `src/pages/Networks.svelte:68`, `src/App.svelte:86`, `src/App.svelte:154`). |
| UI-8.1 targets | PROJECT AUTHORITY — overrides UI-8.1: compact icon buttons are 32×32 CSS px and RowActions uses a 4px gap. GAP: no dedicated touch/mobile target contract established; these measurements do not waive applicable platform accessibility requirements (`src/styles/components.css:86`, `src/components/RowActions.svelte:73`). |
| UI-1.5 loading/empty/error | PROJECT AUTHORITY — overrides UI-1.5: stores own availability/loading, capability helper owns empty-state wording, pages own markup and local attribution. No universal feedback renderer (`src/store.svelte.ts:19`, `src/store/capabilities.svelte.ts:81`, `src/pages/Volumes.svelte:247`, `src/lib/errorReporter.ts:23`). |
| UI-9.1 waiting feedback | GAP: no app-wide skeleton-versus-spinner convention. Dashboard's skeleton branch exists but root passes `loading={false}`; do not describe it as active global policy (`src/pages/Dashboard.svelte:154`, `src/App.svelte:155`). |
| UI-9.2 optimistic updates | PROJECT AUTHORITY — overrides UI-9.2: transfer cancellation marks pending, reconciles outcomes, and restores prior status with visible error on rejection. GAP: no universal optimistic resource-update/rollback policy (`src/components/notifications/NotificationItem.svelte:62`, `src/store/notifications.svelte.ts:270`, `src/lib/settingsStore.svelte.ts:47`). |

Accessibility behavior is component-specific. ModelPicker owns combobox keyboard/ARIA, NotificationPanel owns focus trap/return/Escape, and icon actions require explicit accessible names. Svelte/ESLint accessibility warnings are intentionally filtered, so green mechanical checks do not certify accessibility (`src/components/settings/ModelPicker.svelte:95`, `src/components/notifications/NotificationPanel.svelte:61`, `src/components/RowActions.svelte:43`, `svelte.config.js:1`, `eslint.config.js:124`).

Read `docs/design-system/README.md` when changing tokens or shared visual primitives, and `docs/code-review-rules.md` when reviewing a frontend change. Preserve owner-specific input, event and lifetime contracts rather than generalizing one example to every view.
