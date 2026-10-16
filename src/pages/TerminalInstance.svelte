<script lang="ts">
  import { onMount } from "svelte";
  import { Terminal as XTerm } from "@xterm/xterm";
  // Required, not cosmetic. xterm.js positions its row divs, hides the
  // character-measuring element, and sizes the cursor entirely from this
  // stylesheet. Without it the measure element renders as visible garbage and
  // the cursor draws as an oversized block.
  import "@xterm/xterm/css/xterm.css";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import { uiState } from "../store.svelte";
  import { sessionLabel } from "../lib/terminal-transport";
  import { t } from "../lib/i18n.svelte";
  import {
    exitHint,
    openTerminal,
    type SessionKind,
    type TerminalHandle,
  } from "../lib/terminal-transport";

  let { sessionId, kind, active, termTheme, onFocusExit } = $props<{
    sessionId: string;
    kind: SessionKind;
    active: boolean;
    onFocusExit?: () => void;
    termTheme: NonNullable<ConstructorParameters<typeof XTerm>[0]>["theme"];
  }>();

  let termRef = $state<HTMLDivElement | null>(null);
  let xterm: XTerm | null = null;
  let fit: FitAddon | null = null;
  let handle: TerminalHandle | null = null;
  let exitPollId: ReturnType<typeof setInterval> | null = null;
  let mounted = true;
  let connected = $state(false);
  let error = $state<string | null>(null);
  let ended = $state(false);
  let exitCode = $state<number | null>(null);
  let polling = false;
  function viewLogs() {
    if (kind.kind !== 'containerExec') return;
    uiState.pendingContainerLogs = { containerId: kind.containerId, engine: kind.engine };
    uiState.currentPage = 'containers';
  }
  async function failSession(reason: unknown) {
    if (!mounted || ended) return;
    connected = false; ended = true; error = String(reason);
    if (xterm) xterm.options.disableStdin = true;
    if (exitPollId) clearInterval(exitPollId);
    exitPollId = null;
    await handle?.close().catch(() => {});
  }


  /** Last size pushed to the pty, so identical resizes are not re-sent. */
  let lastRows = 0;
  let lastCols = 0;
  let resizeTimer: ReturnType<typeof setTimeout> | null = null;

  /**
   * Tell the pty its new grid.
   *
   * Debounced because a drag fires continuously, and guarded against 0 —
   * `fit()` reports zero while the element is hidden, and a 0-row pty makes
   * some shells abort.
   */
  function pushResize() {
    if (!xterm || !handle) return;
    const { rows, cols } = xterm;
    if (!rows || !cols) return;
    if (rows === lastRows && cols === lastCols) return;

    lastRows = rows;
    lastCols = cols;
    if (resizeTimer) clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      handle?.resize(rows, cols).catch(() => {});
    }, 50);
  }

  function refit() {
    if (!fit || !active) return;
    try {
      fit.fit();
      pushResize();
    } catch {
      /* element not laid out yet */
    }
  }

  onMount(() => {
    mounted = true;
    if (!termRef) return;

    xterm = new XTerm({
      cursorBlink: true,
      screenReaderMode: true,
      fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
      fontSize: 13,
      lineHeight: 1.4,
      // Bounded: an unbounded scrollback plus a chatty build log is a slow leak.
      scrollback: 5000,
      theme: termTheme,
    });

    fit = new FitAddon();
    xterm.attachCustomKeyEventHandler((event) => {
      if (event.type === 'keydown' && event.ctrlKey && event.shiftKey && event.key === 'F6') {
        event.preventDefault(); onFocusExit?.(); return false;
      }
      return true;
    });
    xterm.loadAddon(fit);
    xterm.loadAddon(new WebLinksAddon());
    xterm.open(termRef);
    xterm.textarea?.setAttribute("aria-label", sessionLabel(kind));
    fit.fit();

    const resizeObserver = new ResizeObserver(() => refit());
    resizeObserver.observe(termRef);

    const connect = async () => {
      // No "connecting" banner on the happy path — the shell prompt is the
      // signal that the session is live, and a banner just pushes it down.
      // Failures still announce themselves below.
      try {
        // The session id is the tab's, with no timestamp mixed in. It used to
        // carry `Date.now()`, so every remount opened a *new* pty and abandoned
        // the old one; keying it to the tab means a remount reattaches.
        handle = await openTerminal(sessionId, kind, (text) => {
          // Written through untouched. The old code rewrote `\n` as `\r\n` to
          // compensate for the `script(1)` wrapper, which corrupts any program
          // that positions the cursor itself.
          xterm?.write(text);
        });

        if (!mounted) {
          await handle.close();
          return;
        }

        connected = true;
        if (active) xterm?.focus();
        // Send the real grid now that the pty exists — it starts at 80x24.
        lastRows = 0;
        lastCols = 0;
        refit();

        xterm!.onData((input) => {
          if (connected && !ended) handle?.write(input).catch(failSession);
        });

        // Output is pushed, so nothing polls for it. This only asks whether the
        // shell has died, so the UI stops showing a live prompt for a dead pty.
        exitPollId = setInterval(async () => {
          if (!mounted || !handle || polling) return;
          polling = true;
          try {
            const code = await handle.pollExit();
            if (code !== null && code !== undefined) {
              xterm?.writeln(
                `\r\n\x1b[33m● ${t("terminal.session_ended", { default: "Session ended (exit code: {code})", code })}\x1b[0m`,
              );
              // Some exit codes are meaningless on their own — a shell-less
              // image just reports 127 and an OCI error. Say what happened.
              const hint = exitHint(kind, code);
              if (hint) xterm?.writeln(`\x1b[36m  ${hint}\x1b[0m`);
              connected = false; ended = true; exitCode = code;
              if (xterm) xterm.options.disableStdin = true;
              await handle.close();
              if (exitPollId) clearInterval(exitPollId);
              exitPollId = null;
            }
          } catch (error) { await failSession(error); }
          finally { polling = false; }
        }, 1000);
      } catch (e) {
        if (!mounted) return;
        xterm!.writeln(
          `\r\n\x1b[31m● ${t("terminal.failed_to_connect", { default: "Failed to connect: {error}", error: String(e) })}\x1b[0m`,
        );
        xterm!.writeln(
          `\x1b[33m  ${kind.kind === "containerExec" ? t("terminal.container_recovery", { default: "Check the engine and container in Containers, then open a new session." }) : t("terminal.ensure_running", { default: "Make sure the instance is running." })}\x1b[0m`,
        );
        error = String(e); ended = true;
      }
    };

    connect();

    return () => {
      mounted = false;
      resizeObserver.disconnect();
      if (exitPollId) clearInterval(exitPollId);
      if (resizeTimer) clearTimeout(resizeTimer);
      handle?.close().catch(() => {});
      handle = null;
      xterm?.dispose();
      xterm = null;
    };
  });

  // Refit when the tab becomes visible: it was measured at zero while hidden.
  $effect(() => {
    if (active) setTimeout(refit, 50);
  });
</script>

<div class="terminal-session" style:display={active ? 'flex' : 'none'}>
  <div class="session-context">
    <span>{sessionLabel(kind)}</span>
    <span role="status" aria-live="polite">{error ? t('terminal.connection_failed', { default: 'Connection failed' }) : ended ? t('terminal.ended', { default: 'Ended' }) : connected ? t('terminal.connected', { default: 'Connected' }) : t('terminal.connecting', { default: 'Connecting…' })}</span>
    <button class="btn btn-ghost" disabled={!connected} onclick={() => xterm?.focus()}>{t('terminal.focus_terminal', { default: 'Focus terminal' })}</button>
    <span>{t('terminal.focus_exit', { default: 'Ctrl+Shift+F6 returns focus to session controls.' })}</span>
  </div>
  {#if error}<p class="session-error" role="alert">{error}</p>{/if}
  {#if ended && exitCode !== null && exitHint(kind, exitCode)}<p class="session-hint">{kind.kind === 'containerExec' ? t('terminal.no_shell', { default: 'This container has no usable shell. View its logs or choose an image with a shell.' }) : exitHint(kind, exitCode)}</p>{/if}
  {#if ended && kind.kind === 'containerExec'}<button class="btn btn-ghost" onclick={viewLogs}>{t('terminal.view_logs', { default: 'View container logs' })}</button>{/if}
  <div bind:this={termRef} class="terminal-output"></div>
</div>

<style>
  .terminal-session { height: 100%; min-height: 0; flex-direction: column; }
  .session-context { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px 16px; padding: 6px 12px; color: var(--text-secondary); font-size: var(--text-sm); overflow-wrap: anywhere; flex-shrink: 0; }
  .session-context span:first-child { color: var(--text-primary); }
  .session-error, .session-hint { margin: 0; padding: 6px 12px; color: var(--text-primary); overflow-wrap: anywhere; }
  .terminal-output { flex: 1; min-height: 0; padding: 4px; }
</style>
