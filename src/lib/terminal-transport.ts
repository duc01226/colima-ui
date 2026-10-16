/**
 * Thin wrapper over the terminal Tauri commands.
 *
 * The terminal used to talk HTTP: one POST per keystroke, a GET polled every
 * 100 ms for output, and a bearer token on each. It is IPC now — no port, no
 * token, and output is pushed rather than polled. Keeping the `invoke` calls
 * here means the component deals in `onOutput` callbacks, not transport.
 */

/**
 * What a session attaches to. Mirrors `SessionKind` in `terminal_session.rs`;
 * the `kind` tag is what serde matches on.
 */
export type SessionKind =
  | { kind: "containerExec"; containerId: string; engine: string; containerName?: string }
  | { kind: "colima"; profile: string }
  | { kind: "lima"; instance: string }
  | { kind: "k8sExec"; namespace: string; pod: string; container: string };

/** Human label for a tab. */
export function sessionLabel(k: SessionKind): string {
  switch (k.kind) {
    case "containerExec":
      return `${k.containerName || k.containerId.slice(0, 12)} · ${k.engine}`;
    case "colima":
      return k.profile === "default" ? "colima" : k.profile;
    case "lima":
      return `🐧 ${k.instance}`;
    case "k8sExec":
      return k.container ? `${k.pod}/${k.container}` : k.pod;
  }
}

/**
 * Explain an exit code when the raw one is useless on its own.
 *
 * `kubectl exec` returns 127 with an OCI runtime error when the requested
 * program is not in the image. For a shell that means the image has none —
 * coredns and other `FROM scratch` / distroless builds ship only their binary.
 * No argv fixes that, so the honest response is to say so and point at what
 * does work.
 *
 * Returns null when the code speaks for itself.
 */
export function exitHint(kind: SessionKind, code: number): string | null {
  if (kind.kind === "containerExec" && (code === 127 || code === 126)) {
    return "This container has no usable shell. Use its logs or choose an image that includes a shell.";
  }
  if (kind.kind === "k8sExec" && code === 127) {
    return (
      "This image has no shell, so there is nothing to attach to. " +
      "Images built FROM scratch or on a distroless base ship only their " +
      "binary. Use the Logs tab to read output, or attach an ephemeral " +
      "container with: kubectl debug -it " +
      `-n ${kind.namespace} ${kind.pod} --image=busybox --target=` +
      `${kind.container || "<container>"}`
    );
  }
  return null;
}

export interface TerminalHandle {
  write(data: string): Promise<void>;
  resize(rows: number, cols: number): Promise<void>;
  /** Resolves with the exit code once the shell has died, else null. */
  pollExit(): Promise<number | null>;
  close(): Promise<void>;
}

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
type ChannelCtor = new <T>() => { onmessage: (msg: T) => void };

async function tauri(): Promise<{ invoke: Invoke; Channel: ChannelCtor }> {
  const core = await import("@tauri-apps/api/core");
  return {
    invoke: core.invoke as Invoke,
    Channel: core.Channel as unknown as ChannelCtor,
  };
}

/** Capture backend-verified origin; the name is display-only metadata. */
export async function containerTerminalTarget(containerId: string, containerName: string): Promise<SessionKind> {
  const { invoke } = await tauri();
  const kind = await invoke<SessionKind>("terminal_target", { containerId });
  if (kind.kind !== "containerExec" || kind.containerId !== containerId || !kind.engine) {
    throw new Error("Invalid container terminal target");
  }
  return { ...kind, containerName };
}

/**
 * Open a session. `onOutput` is called with decoded text as the shell produces
 * it; the backend coalesces on a frame boundary, so this fires at most ~60/s
 * regardless of how loud the command is.
 */
export async function openTerminal(
  sessionId: string,
  kind: SessionKind,
  onOutput: (text: string) => void,
): Promise<TerminalHandle> {
  const { invoke, Channel } = await tauri();

  const channel = new Channel<string>();
  channel.onmessage = onOutput;

  await invoke<void>("terminal_create", {
    sessionId,
    kind,
    onOutput: channel,
  });

  return {
    write: (data) => invoke<void>("terminal_write", { sessionId, data }),
    resize: (rows, cols) => invoke<void>("terminal_resize", { sessionId, rows, cols }),
    pollExit: () => invoke<number | null>("terminal_poll_exit", { sessionId }),
    close: () => invoke<void>("terminal_close", { sessionId }),
  };
}
