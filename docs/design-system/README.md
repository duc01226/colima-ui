# Design system source guide

> Last scanned: 2026-10-14

Read this before changing tokens, shared controls, icons or visual composition. This is a source-derived guide to existing owners; it does not establish a new product design authority or certify visual/accessibility conformance.

## Authority and consumption

The maintained visual overview is the CSS Design System section of `docs/frontend.md:252`. Read `docs/topology-design-philosophy.md` when changing topology node/edge kinds or node actions: that guide explicitly owns the topology interaction rules (`docs/topology-design-philosophy.md:3`, `docs/topology-design-philosophy.md:83`).

The stylesheet chain is direct source consumption: `index.html:12` loads `src/main.ts`, which imports bundled font CSS and `src/index.css`. The latter is imports-only and orders tokens → reset → components → layout → forms → overlays → features. Put shared rules in the corresponding owner module and component-specific rules in its component style block (`src/main.ts:1`, `src/main.ts:14`, `src/index.css:1`, `src/index.css:13`). The token check validates reads; it does not generate token CSS or certify visual quality (`scripts/check-css-tokens.mjs:12`, `scripts/check-css-tokens.mjs:38`).

Shared native controls use CSS classes rather than a universal Button/Input Svelte component. Reuse an existing owner only when its contract matches the new interaction. The shared browser/native Svelte shell consumes these same sources (`src/styles/components.css:20`, `src/styles/forms.css:3`, `src/App.svelte:28`).

## Shared component and asset owners

| Area | Owner, variants and constraints |
| --- | --- |
| Shell/navigation | `src/App.svelte:137`, `src/components/Sidebar.svelte:393` — page selection and persistent panels; panel toggle state differs from page selection. |
| Row/menu actions | `src/components/RowActions.svelte:6`, `src/components/ContextMenu.svelte:4` — inline/overflow, tone, disabled/divider and keyboard-active variants. Feature owner supplies actions. |
| Settings layout and model input | `src/components/settings/SettingsSection.svelte:8`, `src/components/settings/ModelPicker.svelte:16` — child snippet/optional element handle; editable filtered models with keyboard and fetching/empty states. |
| Feedback and dialogs | `src/components/ConfirmDialog.svelte:13`, `src/components/ToastContainer.svelte:35`, `src/components/notifications/NotificationPanel.svelte:61` — severity-specific feedback; modal focus behavior belongs to the actual component, not the CSS class. |
| Domain display | `src/components/activity/Sparkline.svelte:10`, `src/components/security/RuleChecklist.svelte:53`, `src/components/security/RuleChecklist.svelte:94`, `src/components/topology/NodeDetail.svelte:84` — owner-specific chart gaps, rule evidence/remediation text with optional guide navigation, and graph/list identity conversion. |
| Change presentation | `src/components/DiffView.svelte:5`, `src/components/UnifiedDiffView.svelte:2` — structured fields and text patches are distinct data shapes; renderers do not apply changes. |
| Icons and bundled fonts | `src/components/Icons.svelte:1` exports trusted SVG strings; `src/components/Icon.svelte:2` wraps name/size/color. Fonts are bundled through `src/main.ts:1`. Raw markup must come from the trusted internal icon owner, not untrusted text (`src/components/ContextMenu.svelte:92`). |

Accessibility is scoped: RowActions supplies accessible icon names; ModelPicker supplies combobox/listbox semantics and keyboard handling; NotificationPanel traps/restores focus and handles Escape. These guarantees are not inherited automatically by all dialogs or custom controls (`src/components/RowActions.svelte:43`, `src/components/settings/ModelPicker.svelte:95`, `src/components/notifications/NotificationPanel.svelte:61`).

Read `docs/frontend-patterns-reference.md` when changing form behavior, loading/error feedback or component lifetime. Source comments/Props/JSDoc and existing technical guides document the current owners; the inspected repository does not establish a separate published component package or Storybook contract.

## Token source and conventions

Edit global theme declarations in `src/styles/tokens.css:3`; `.tauri-app` overrides header geometry in the same file. Consumers' `var()` reads are derived usage. Notification `--row-accent` is local semantic mapping; table `--cols`/`--col-N` are interaction geometry. Xterm has a separate literal palette and font configuration rather than generated CSS-token consumption (`src/styles/tokens.css:145`, `src/components/notifications/NotificationItem.svelte:215`, `src/pages/Containers.svelte:546`, `src/lib/columnResize.ts:10`, `src/pages/Terminal.svelte:21`, `src/pages/TerminalInstance.svelte:77`).

The following records capture applicable UI clauses. Project conventions specialize the shared defaults; gaps retain the applicable standard/platform baseline. Fixed step counts, spacing units and motion durations in framework examples do not require replacing this project's declarations.

| Clause | Project authority or gap |
| --- | --- |
| UI-2.5 type | PROJECT AUTHORITY — overrides UI-2.5 examples: `--text-xs .7rem`, `--text-sm .8125rem`, `--text-base .875rem`, `--text-lg 1rem`, `--text-md` aliases lg, `--text-xl 1.125rem`, `--text-2xl 1.375rem`, `--text-3xl 1.75rem`. Families are Inter/system sans and JetBrains Mono/monospace (`src/styles/tokens.css:119`, `src/styles/tokens.css:124`). |
| UI-4.1 spacing | PROJECT AUTHORITY for shell geometry: sidebar 240px/collapsed 64px; browser header 68px with 16px top/bottom padding, Tauri 92px with 44px/12px. GAP — no named general spacing base; UI-4.1 consistency guidance applies. Shared controls use owner-local literal padding (`src/styles/tokens.css:98`, `src/styles/tokens.css:145`, `src/styles/components.css:24`, `src/styles/forms.css:4`). |
| UI-3.2 accents | PROJECT AUTHORITY — overrides single-accent shorthand with seven base colors and six glow companions. Blue owns action/focus/selection roles; green success; red danger/error; yellow warning; purple tier/metric/graph visuals; orange severity/resource visuals. Cyan and glow declarations have no maintained-source consumers found; do not infer a job from declaration alone (`src/styles/tokens.css:32`, `src/styles/tokens.css:47`, `src/styles/components.css:43`, `src/styles/components.css:61`, `src/styles/components.css:153`, `src/styles/components.css:743`, `src/components/notifications/NotificationItem.svelte:253`, `src/components/topology/GraphCanvas.svelte:484`). |
| UI-3.1 contrast | PROJECT AUTHORITY for declared colors/local metadata choice; GAP — no project-wide numeric contrast policy. UI-3.1 standard selection applies. Measured opaque pairs are below; notification metadata deliberately uses secondary instead of muted. Text benchmark is 4.5:1, meaningful edges 3:1, subject to actual context (`src/styles/tokens.css:5`, `src/styles/tokens.css:14`, `src/styles/tokens.css:28`, `src/components/notifications/NotificationItem.svelte:330`). |
| UI-3.4 dark surfaces | PROJECT AUTHORITY — overrides defaults with lifted dark app/content/card/hover surfaces, elevation aliases/shadows and softened primary text `#e6edf3`. No light branch was found in the maintained CSS scope; xterm ANSI bright white is a separate terminal role (`src/styles/tokens.css:4`, `src/styles/tokens.css:28`, `src/styles/tokens.css:72`, `src/styles/tokens.css:90`, `src/pages/Terminal.svelte:21`). |
| UI-5.4 motion | PROJECT AUTHORITY — overrides timing examples: fast 150ms, normal 250ms, slow 400ms, all `ease`. Local motion also uses other curves/durations. Reduced-motion rules are component/page scoped; GAP — no global reduced-motion override. Preserve applicable reduced-motion requirements when extending motion (`src/styles/tokens.css:135`, `src/styles/features.css:3`, `src/styles/overlays.css:25`, `src/components/notifications/NotificationPanel.svelte:358`, `src/components/ToastContainer.svelte:78`, `src/pages/Containers.svelte:901`). |

## Bounded contrast measurements

These ratios were computed from declared opaque sRGB token values, rounded to two decimals. They record pairs and do not establish app-wide conformance or a production defect. Alpha, gradient, backdrop and mixed-state contexts are **unmeasured**; do not reuse an opaque ratio for them (`src/styles/tokens.css:5`, `src/styles/tokens.css:14`, `src/styles/tokens.css:28`, `src/styles/tokens.css:33`).

| Foreground / background | Ratio | Source usage |
| --- | --- | --- |
| `--text-primary / --bg-app` | 16.33:1 | Body (`src/styles/reset.css:18`) |
| `--text-secondary / --bg-card` | 5.84:1 | Notification metadata (`src/components/notifications/NotificationItem.svelte:335`) |
| `--text-muted / --bg-input` | 3.10:1 | Placeholder (`src/styles/forms.css:26`) |
| `--text-muted / --bg-content` | 2.90:1 | Table heading (`src/styles/components.css:230`) |
| `--text-muted / --bg-card` | 2.75:1 | Documented alternative the metadata owner avoids (`src/components/notifications/NotificationItem.svelte:330`) |
| `--border-primary / --bg-card` | 1.34:1 | Card boundary (`src/styles/components.css:4`) |
| `--accent-blue / --bg-input` | 7.44:1 | Focus border (`src/styles/forms.css:21`) |

Browser favicon/sidebar logo use `public/colima_icon.png`; desktop asset selection belongs to Tauri bundle config. No editable master or asset-generation contract was established (`index.html:5`, `src/components/Sidebar.svelte:370`, `src-tauri/tauri.conf.json:60`).

Keep global declarations, local derived values and xterm's palette distinct. Reuse a visual owner with matching interaction/accessibility behavior, and verify the actual state and backdrop before treating a token measurement as a rendered result.
