<script lang="ts">
  import { capabilitiesState, isDockerOnlyHost, supportsHostVms, supportsKubernetes } from "../store/capabilities.svelte";
  import { engineConnectionState } from "../lib/engineConnection.svelte";
  import { refetchAllResources } from "../lib/dataPoller";
  import { onMount } from "svelte";
  import { dashboardState, dockerState, uiState } from "../store.svelte";
  import Icon from "../components/Icon.svelte";
  import { t } from "../lib/i18n.svelte";
  import { k8sApi, dockerApi } from "../lib/api";
  import { parseItems } from "../lib/k8sUtils";
  import { containerTerminalTarget, sessionLabel, type SessionKind } from "../lib/terminal-transport";
  import { reportError } from "../lib/errorReporter";
  import { isRunningInTauri } from "../lib/env";
  import TerminalInstance from "./TerminalInstance.svelte";

  // The API origin and bearer token used to be threaded down to every tab,
  // because each keystroke was an authenticated HTTP request. Sessions run over
  // Tauri IPC now, so there is no origin to resolve and no token to pass.

  interface TerminalTab {
    id: string;
    label: string;
    kind: SessionKind;
  }

  const termTheme = {
    background: "#0D1117",
    foreground: "#E6EDF3",
    cursor: "#58A6FF",
    selectionBackground: "rgba(88, 166, 255, 0.3)",
    black: "#0D1117",
    red: "#F85149",
    green: "#3FB950",
    yellow: "#D29922",
    blue: "#58A6FF",
    magenta: "#BC8CFF",
    cyan: "#39D2C0",
    white: "#E6EDF3",
    brightBlack: "#6E7681",
    brightRed: "#F85149",
    brightGreen: "#3FB950",
    brightYellow: "#D29922",
    brightBlue: "#58A6FF",
    brightMagenta: "#BC8CFF",
    brightCyan: "#39D2C0",
    brightWhite: "#FFFFFF",
  };

  let tabs = $state<TerminalTab[]>([]);
  let activeTab = $state<string | null>(null);
  let showPicker = $state(false);
  let picker = $state<HTMLDialogElement | null>(null);
  let newSessionButton = $state<HTMLButtonElement | null>(null);
  let opening = $state(false);
  let openRequest = 0;
  let dockerLoading = $state(false);
  let dockerError = $state('');
  let dockerLoaded = $state(false);
  const currentDockerTargets = $derived(!!capabilitiesState.platform && (!isDockerOnlyHost() ||
    (!!engineConnectionState.snapshot?.active.ready && dockerState.originGeneration !== null && dockerState.originGeneration === engineConnectionState.snapshot.active.generation)));
  let containers = $derived((currentDockerTargets ? dockerState.containers : []).filter(c => c.State === 'running'));
  const targetError = $derived(!capabilitiesState.platform ? t('connection.platform_checking') : isDockerOnlyHost()
    ? dockerError || dockerState.error || (currentDockerTargets ? '' : t('connection.unavailable_title')) : dockerError);
  const targetsLoaded = $derived(isDockerOnlyHost() ? currentDockerTargets && !dockerState.error : dockerLoaded);

  async function loadDockerTargets() {
    if (dockerLoading || !capabilitiesState.platform) return;
    dockerLoading = true; dockerError = '';
    try {
      if (isDockerOnlyHost()) await refetchAllResources();
      else { dockerState.containers = await dockerApi.listContainers(true); dockerLoaded = true; }
    }
    catch (error) { dockerError = String(error); }
    finally { dockerLoading = false; }
  }
  async function openDocker(c: { Id: string; Names: string }) {
    if (opening) return;
    opening = true;
    const request = ++openRequest;
    try { const kind = await containerTerminalTarget(c.Id, c.Names); if (request === openRequest) openSession(kind); }
    catch (error) { dockerError = String(error); reportError(error, { action: t('terminal.open_container', { default: 'Open terminal' }) }); }
    finally { opening = false; }
  }
  $effect(() => {
    if (!showPicker || !picker) return;
    const dialog = picker;
    dialog.showModal();
    return () => { dialog.close(); newSessionButton?.focus(); };
  });
  function closePicker() { ++openRequest; showPicker = false; }


  let instances = $derived((supportsHostVms() ? dashboardState.colimaInstances : []).filter(i => i.status === "Running"));
  let limaVMs = $derived((supportsHostVms() ? dashboardState.linuxVMs : []).filter(v => v.status === "Running"));

  /**
   * Container targets from the connected cluster.
   *
   * The picker used to list only Colima/Lima VMs, so on a machine whose
   * Kubernetes comes from somewhere else — OrbStack, Docker Desktop, a remote
   * context — the Terminal page offered nothing to open, even though the
   * terminal itself can attach to any container. Loaded lazily: the page should
   * not pay for a cluster query nobody asked for.
   */
  interface PodTarget {
    namespace: string;
    pod: string;
    container: string;
    label: string;
  }

  let podTargets = $state<PodTarget[]>([]);
  let podsLoading = $state(false);
  let podsError = $state("");

  // On mount, not on picker-open: `hasRunning` gates whether the New Session
  // button renders at all, so the cluster has to be counted before the user
  // reaches for it.
  onMount(() => { loadPodTargets(); loadDockerTargets(); });

  async function loadPodTargets() {
    if (!supportsKubernetes()) return;
    if (podsLoading) return;
    podsLoading = true;
    podsError = "";
    try {
      const pods = parseItems(await k8sApi.pods("")).filter(
        (p) => p.status === "Running",
      );

      podTargets = pods.flatMap((p) => {
        const rawSpec = p._raw.spec as { containers?: { name?: string }[] } | undefined;
        const containers: string[] = rawSpec?.containers?.map((c) => c.name ?? "") ?? [];
        const ns = p.namespace || "default";

        // One row per container only when there is a choice to make; a
        // single-container pod does not need the name spelled out.
        if (containers.length <= 1) {
          return [{ namespace: ns, pod: p.name, container: "", label: `${ns}/${p.name}` }];
        }
        return containers.map((c) => ({
          namespace: ns,
          pod: p.name,
          container: c,
          label: `${ns}/${p.name} · ${c}`,
        }));
      });
    } catch (e) {
      // No cluster is a normal state, not an error worth a toast.
      podsError = String(e);
      podTargets = [];
    } finally {
      podsLoading = false;
    }
  }

  /** Anything at all that a session could attach to. */
  let hasRunning = $derived(
    containers.length > 0 || instances.length > 0 || limaVMs.length > 0 || podTargets.length > 0,
  );

  function addTab(profile: string, vmType: "colima" | "lima" = "colima") {
    openSession(
      vmType === "lima"
        ? { kind: "lima", instance: profile }
        : { kind: "colima", profile },
    );
  }

  /**
   * Skip the picker when there is only one thing to pick.
   *
   * Counts every source, not just VMs — otherwise a machine with one pod and no
   * VM would open a dialog containing a single row.
   */
  function openOrPick() {
    const total = containers.length + instances.length + limaVMs.length + podTargets.length;
    if (total !== 1) {
      showPicker = true;
      return;
    }

    if (containers.length === 1) {
      void openDocker(containers[0]);
    } else if (instances.length === 1) {
      const name = instances[0].name;
      addTab(name === "colima" ? "default" : name.replace("colima-", ""), "colima");
    } else if (limaVMs.length === 1) {
      addTab(limaVMs[0].name, "lima");
    } else {
      openPod(podTargets[0]);
    }
  }

  function openPod(p: PodTarget) {
    openSession({
      kind: "k8sExec",
      namespace: p.namespace,
      pod: p.pod,
      container: p.container,
    });
  }

  /**
   * Open a tab for any session kind.
   *
   * The id is minted once here and never re-derived, so remounting a tab
   * reattaches to its existing pty instead of spawning a second one.
   */
  function openSession(kind: SessionKind) {
    const id = `term-${crypto.randomUUID()}`;
    tabs = [...tabs, { id, label: sessionLabel(kind), kind }];
    activeTab = id;
    showPicker = false;
  }

  // Consume the one-shot deep link set by `openTerminalSession` — today the
  // Kubernetes pod drawer's shell button. Cleared immediately so returning to
  // this page later does not reopen a stale session.
  $effect(() => {
    const pending = uiState.pendingTerminalSession as SessionKind | null;
    if (!pending) return;
    uiState.pendingTerminalSession = null;
    openSession(pending);
  });

  function removeTab(id: string) {
    const next = tabs.filter(t => t.id !== id);
    if (activeTab === id) {
      activeTab = next.length > 0 ? next[next.length - 1].id : null;
    }
    tabs = next;
  }
</script>

<div class="content-header" data-tauri-drag-region>
  <h1>{t('terminal.title', { default: 'Terminal' })}</h1>
  <div class="content-header-actions">
    <button bind:this={newSessionButton} class="btn btn-primary" disabled={opening || !isRunningInTauri()} onclick={openOrPick}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>
      </svg>
      {t('terminal.new_session', { default: 'New Session' })}
    </button>
  </div>
</div>

<!-- Tab Bar -->
{#if tabs.length > 0}
  <div style="display: flex; align-items: center; gap: 0; border-bottom: 1px solid var(--border-primary); background: var(--bg-content); padding-left: 12px; flex-shrink: 0; overflow: auto;">
    {#each tabs as tab (tab.id)}
      <div class="terminal-tab">
        <button class="btn btn-ghost" class:selected={activeTab === tab.id} aria-pressed={activeTab === tab.id} onclick={() => activeTab = tab.id}>{tab.label}</button>
        <button class="btn btn-icon btn-ghost" aria-label={t('terminal.close_named', { default: 'Close terminal for {name}', name: tab.label })} onclick={() => { removeTab(tab.id); newSessionButton?.focus(); }}><Icon name="Close" size={14} /></button>
      </div>
    {/each}
  </div>
{/if}

<!-- Terminal Content -->
<div style="flex: 1; overflow: hidden; background: #0D1117; display: flex; flex-direction: column;">
  {#if tabs.length === 0}
    <div class="empty-state" style="height: 100%;">
      <div class="empty-state-icon">
        <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" style="color: var(--text-muted);">
          <polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/>
        </svg>
      </div>
      <div class="empty-state-title">{t('terminal.no_sessions', { default: 'No terminal sessions' })}</div>
      <div class="empty-state-text">
        {t('terminal.instructions', { default: 'Open a terminal in a running container or choose another available target.' })}
      </div>
      {#if hasRunning}
        <button class="btn btn-primary" onclick={() => {
          openOrPick();
        }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>
          </svg>
          {t('terminal.new_session', { default: 'New Session' })}
        </button>
      {:else}
        <p style="font-size: var(--text-xs); color: var(--text-muted); line-height: 1.7;">
          {#if podsLoading}
            Looking for targets…
          {:else}
            {t('terminal.no_targets', { default: 'No running targets. Start a container in Containers, then open its terminal.' })}
          {/if}
        </p>
      {/if}
    </div>
  {:else}
    {#each tabs as tab (tab.id)}
      <div style="display: {activeTab === tab.id ? 'block' : 'none'}; flex: 1; position: relative;">
        <TerminalInstance
          sessionId={tab.id}
          kind={tab.kind}
          active={activeTab === tab.id}
          {termTheme}
          onFocusExit={() => newSessionButton?.focus()}
        />
      </div>
    {/each}
  {/if}
</div>

<!-- The native modal dialog owns inert background, Escape and focus containment. -->
{#if showPicker}
  <dialog bind:this={picker} class="modal terminal-picker" oncancel={closePicker} onclose={closePicker} aria-labelledby="terminal-picker-title">
    <div class="modal-header">
      <h2 id="terminal-picker-title" class="modal-title">{t('terminal.select_target', { default: 'Select a terminal target' })}</h2>
      <button class="btn btn-ghost" onclick={closePicker}>{t('common.cancel', { default: 'Cancel' })}</button>
    </div>
    <h3>{t('sidebar.containers', { default: 'Containers' })}</h3>
    {#if dockerLoading}<p role="status">{t('terminal.loading_containers', { default: 'Loading Docker containers…' })}</p>{/if}
    {#if targetError}<p role="alert">{targetError}</p><button class="btn btn-ghost" onclick={loadDockerTargets}>{t('common.retry', { default: 'Retry' })}</button>{/if}
    {#each containers as c (c.Id)}
      <button class="btn btn-ghost target-button" disabled={opening} onclick={() => openDocker(c)}>{c.Names} <span>{c.Id.slice(0, 12)}</span></button>
    {/each}
    {#if targetsLoaded && !targetError && containers.length === 0}<p>{t('terminal.no_containers', { default: 'No running containers. Open Containers to start one.' })}</p>{/if}
    {#each instances as inst (inst.name)}<button class="btn btn-ghost target-button" onclick={() => addTab(inst.name === 'colima' ? 'default' : inst.name.replace('colima-', ''), 'colima')}>Colima · {inst.name}</button>{/each}
    {#each limaVMs as vm (vm.name)}<button class="btn btn-ghost target-button" onclick={() => addTab(vm.name, 'lima')}>Lima · {vm.name}</button>{/each}
    {#each podTargets as p (p.label)}<button class="btn btn-ghost target-button" onclick={() => openPod(p)}>Kubernetes · {p.label}</button>{/each}
    {#if podsError}<p>{t('terminal.cluster_unavailable', { default: 'Cluster targets are unavailable.' })}</p>{/if}
    {#if opening}<p role="status">{t('terminal.checking_target', { default: 'Checking container and engine…' })}</p>{/if}
  </dialog>
{/if}

<style>
  .terminal-tab { display: flex; align-items: center; gap: 4px; }
  .selected { border-bottom-color: var(--accent-blue); color: var(--text-primary); }
  .terminal-picker { width: min(440px, 90vw); max-height: 80vh; overflow: auto; color: var(--text-primary); background: var(--bg-card); }
  .terminal-picker::backdrop { background: var(--bg-overlay); }
  .target-button { width: 100%; justify-content: space-between; white-space: normal; text-align: left; margin-bottom: 4px; overflow-wrap: anywhere; }
  .target-button span, .terminal-picker p { color: var(--text-secondary); }
  .terminal-picker h3 { font-size: var(--text-sm); }
</style>
