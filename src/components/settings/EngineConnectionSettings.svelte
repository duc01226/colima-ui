<script lang="ts">
  import { onMount, tick } from "svelte";
  import { t } from "../../lib/i18n.svelte";
  import { transferApi } from "../../lib/api/transfer";
  import { capability, capabilityNotice } from "../../store/capabilities.svelte";
  import { openNotificationPanel } from "../../store/notifications.svelte";
  import {
    engineConnectionState as connection, changeEngineDraft, loadEngineConnection,
    checkEngineDraft, saveEngineDraft, restartForEngine,
  } from "../../lib/engineConnection.svelte";

  let { id, onConnected } = $props<{ id: string; onConnected?: () => void }>();
  let dialog: HTMLDialogElement;
  let restartButton = $state<HTMLButtonElement>();
  let checkingTransfers = $state(false);
  let activeTransfers = $state(0);
  let transferError = $state(false);
  let mounted = false;
  let dialogEpoch = 0;
  const active = $derived(connection.snapshot?.active);
  const pending = $derived(connection.snapshot?.pending);
  const contexts = $derived(connection.snapshot?.contexts ?? []);
  const draft = $derived(connection.draft ?? "");
  const checked = $derived(connection.checked?.ready && connection.checkedContext === draft);
  const currentReady = $derived(active?.ready && active.context === draft);
  const unknownDraft = $derived(draft && !contexts.some((c) => c.name === draft));

  onMount(() => {
    mounted = true;
    void loadEngineConnection();
    return () => { mounted = false; dialogEpoch++; dialog?.close(); };
  });

  async function prepareRestart() {
    const epoch = ++dialogEpoch;
    checkingTransfers = true;
    transferError = false;
    dialog.showModal();
    await tick();
    dialog.querySelector<HTMLButtonElement>("button")?.focus();
    try {
      const jobs = await transferApi.list();
      if (mounted && epoch === dialogEpoch) activeTransfers = jobs.filter((j) => j.status === "starting" || j.status === "running").length;
    } catch { if (mounted && epoch === dialogEpoch) transferError = true; }
    finally { if (mounted && epoch === dialogEpoch) checkingTransfers = false; }
  }

  function closeRestart() {
    dialogEpoch++;
    dialog.close();
    restartButton?.focus();
  }

  function viewTransfers() {
    closeRestart();
    openNotificationPanel();
  }

  async function restart() {
    try { await restartForEngine(); }
    catch { if (mounted) closeRestart(); }
  }
</script>

<div class="engine-settings" aria-busy={connection.loading || connection.restarting}>
  <p class="engine-origin">{t("connection.active", { engine: active?.context ?? t("connection.unknown_engine"), default: "Active engine: {engine}" })}</p>
  <p role="status">{connection.loading ? t("connection.loading") : active?.ready
    ? t("connection.connected", { engine: active.context ?? "Docker" })
    : t("connection.unavailable", { engine: active?.context ?? "Docker" })}</p>

  <div class="form-group">
    <label for={id}>{t("connection.next_launch", { default: "Next launch" })}</label>
    <select id={id} class="input select" value={draft} disabled={connection.saving || connection.restarting}
      aria-describedby={`${id}-help ${id}-error`}
      onchange={(event) => changeEngineDraft(event.currentTarget.value)}>
      <option value="">{t("connection.choose")}</option>
      {#if unknownDraft}<option value={draft}>{draft} ({t("connection.selection_missing")})</option>{/if}
      {#each contexts as choice (choice.name)}
        <option value={choice.name} disabled={!choice.supported}>{choice.name}{choice.supported ? "" : ` (${t("connection.unsupported_title")})`}</option>
      {/each}
    </select>
    <p id={`${id}-help`}>{t("connection.scope")}</p>
  </div>

  <div class="engine-actions">
    <button type="button" class="btn btn-ghost" disabled={connection.loading || connection.restarting} onclick={() => void loadEngineConnection()}>{t("connection.refresh")}</button>
    <button type="button" class="btn btn-ghost" disabled={!draft || connection.checking || connection.saving || connection.restarting} onclick={() => void checkEngineDraft()}>
      {connection.checking ? t("connection.checking", { engine: draft }) : t("connection.check")}
    </button>
    {#if onConnected && currentReady}
      <button type="button" class="btn btn-primary" onclick={onConnected}>{t("connection.continue")}</button>
    {:else}
      <button type="button" class="btn btn-primary" disabled={!checked || connection.saving || connection.restarting || draft === pending}
        onclick={() => void saveEngineDraft().catch(() => {})}>{connection.saving ? t("connection.saving") : t("connection.save")}</button>
    {/if}
  </div>

  {#if checked}<p role="status">{t("connection.candidate_ready", { engine: connection.checkedContext })}</p>{/if}
  <div id={`${id}-error`}>
    {#if connection.error}<p role="alert">{t("connection.action_failed")} {connection.error}</p>{/if}
    {#if connection.snapshot?.discovery_error}<p>{t("connection.discovery_failed")} {connection.snapshot.discovery_error}</p>{/if}
    {#if active?.error}<p>{active.error}</p>{/if}
  </div>

  {#if pending}
    <p class="engine-origin">{t("connection.saved", { engine: pending })}</p>
    <button type="button" class="btn btn-ghost" bind:this={restartButton} disabled={connection.restarting} onclick={prepareRestart}>{t("connection.restart")}</button>
  {/if}
  {#if connection.restarting}<p role="status">{t("connection.cleaning")}</p>{/if}

  <details>
    <summary>{t("connection.details")}</summary>
    <dl><dt>{t("connection.endpoint")}</dt><dd>{active?.endpoint ?? "—"}</dd>
      <dt>{t("connection.identity")}</dt><dd>{active?.engine_id ?? "—"}</dd></dl>
    {#each ["docker", "docker-compose"] as id (id)}
      {@const cap = capability(id)}
      {#if cap}<p>{cap.state === "running" ? `${cap.name}: ${cap.version ?? "✓"}` : capabilityNotice(cap).text}</p>{/if}
    {/each}
  </details>
</div>

<dialog bind:this={dialog} aria-labelledby={`${id}-restart-title`} oncancel={(event) => { event.preventDefault(); if (!connection.restarting) closeRestart(); }}>
  <h2 id={`${id}-restart-title`}>{t("connection.restart")}</h2>
  <p>{t("connection.restart_message", { engine: pending ?? active?.context ?? "Docker" })}</p>
  {#if checkingTransfers}<p role="status">{t("connection.checking_transfers")}</p>
  {:else if transferError}<p role="alert">{t("connection.transfer_check_failed")}</p>
  {:else if activeTransfers > 0}<p role="status">{t("connection.transfers_block")}</p>{/if}
  {#if connection.restarting}<p role="status">{t("connection.cleaning")}</p>{/if}
  <div class="engine-actions">
    <button type="button" class="btn btn-ghost" disabled={connection.restarting} onclick={closeRestart}>{t("connection.keep_working")}</button>
    {#if activeTransfers > 0 || transferError}
      <button type="button" class="btn btn-ghost" disabled={connection.restarting} onclick={viewTransfers}>{t("connection.view_transfers")}</button>
    {:else}
      <button type="button" class="btn btn-primary" disabled={checkingTransfers || connection.restarting} onclick={restart}>{t("connection.restart")}</button>
    {/if}
  </div>
</dialog>

<style>
  .engine-settings { min-width: 0; color: var(--text-secondary); }
  p { margin: 8px 0; line-height: 1.6; overflow-wrap: anywhere; }
  .engine-origin, label, h2 { color: var(--text-primary); }
  .engine-origin { font-weight: 600; }
  .engine-actions { display: flex; flex-wrap: wrap; gap: 8px; margin: 12px 0; }
  .btn { white-space: normal; overflow: visible; }
  select { min-width: 0; }
  summary { cursor: pointer; margin-top: 16px; }
  summary:focus-visible { outline: 2px solid var(--accent-blue); outline-offset: 2px; }
  dd { margin: 4px 0 12px; overflow-wrap: anywhere; font-family: var(--font-mono); }
  dialog { background: var(--bg-card); color: var(--text-primary); border: 1px solid var(--border-primary); border-radius: var(--radius-lg); padding: 20px; width: min(480px, calc(100% - 32px)); max-height: calc(100% - 32px); overflow: auto; }
  dialog::backdrop { background: var(--bg-overlay); }
  h2 { font-size: var(--text-xl); margin: 0; }
</style>
