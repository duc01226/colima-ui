import { connectionApi, type ConnectionSettings, type EngineStatus } from "./api/system";
import { redact } from "./redact";

export const engineConnectionState = $state({
  snapshot: null as ConnectionSettings | null,
  loading: false,
  error: "",
  draft: null as string | null,
  checked: null as EngineStatus | null,
  checkedContext: "",
  checking: false,
  saving: false,
  restarting: false,
});

let statusEpoch = 0;
let candidateEpoch = 0;
let inflight: Promise<void> | null = null;

export function changeEngineDraft(context: string): void {
  candidateEpoch++;
  engineConnectionState.draft = context;
  engineConnectionState.checked = null;
  engineConnectionState.checkedContext = "";
  engineConnectionState.error = "";
}

export function markEngineUnavailable(): void {
  statusEpoch++;
  inflight = null;
  engineConnectionState.loading = false;
  const active = engineConnectionState.snapshot?.active;
  if (active) { active.ready = false; active.generation = null; }
}

export function loadEngineConnection(): Promise<void> {
  if (inflight) return inflight;
  const epoch = ++statusEpoch;
  engineConnectionState.loading = true;
  const request = (async () => {
    try {
      const result = await connectionApi.get();
      if (epoch !== statusEpoch) return;
      engineConnectionState.snapshot = result;
      engineConnectionState.draft ??= result.pending ?? result.active.context ?? "";
      engineConnectionState.error = "";
    } catch (error) {
      if (epoch !== statusEpoch) return;
      engineConnectionState.error = redact(String(error));
      if (engineConnectionState.snapshot) engineConnectionState.snapshot.active.ready = false;
    } finally {
      if (epoch === statusEpoch) engineConnectionState.loading = false;
    }
  })();
  inflight = request;
  void request.finally(() => { if (inflight === request) inflight = null; });
  return request;
}

export async function checkEngineDraft(): Promise<void> {
  if (engineConnectionState.checking) return;
  const context = engineConnectionState.draft ?? "";
  const epoch = ++candidateEpoch;
  engineConnectionState.checking = true;
  engineConnectionState.error = "";
  engineConnectionState.checked = null;
  try {
    const result = await connectionApi.check(context);
    if (epoch !== candidateEpoch) return;
    engineConnectionState.checked = result;
    engineConnectionState.checkedContext = context;
  } catch (error) {
    if (epoch === candidateEpoch) engineConnectionState.error = redact(String(error));
  } finally { engineConnectionState.checking = false; }
}

export async function saveEngineDraft(): Promise<void> {
  if (engineConnectionState.saving) return;
  const context = engineConnectionState.draft ?? "";
  if (!engineConnectionState.checked?.ready || engineConnectionState.checkedContext !== context) {
    throw new Error("Check this selection before saving");
  }
  engineConnectionState.saving = true;
  engineConnectionState.error = "";
  try {
    await connectionApi.save(context);
    if (engineConnectionState.snapshot) engineConnectionState.snapshot.pending = context;
  } catch (error) {
    engineConnectionState.error = redact(String(error));
    throw error;
  } finally { engineConnectionState.saving = false; }
}

export async function restartForEngine(): Promise<void> {
  engineConnectionState.restarting = true;
  engineConnectionState.error = "";
  try { await connectionApi.restart(false); }
  catch (error) {
    engineConnectionState.error = redact(String(error));
    engineConnectionState.restarting = false;
    throw error;
  }
}
