import { colimaApi, dockerApi, volumesApi, networksApi, sysMethods, getApiToken, type ColimaInstance } from "./api";
import { resolveApiBase } from "./api/client";
import { loadCapabilities, isDockerOnlyHost, supportsHostVms, pageAvailable } from "../store/capabilities.svelte";
import { normalizeContainer, normalizeImage } from "./normalizers";
import { dockerState, resourceState, dashboardState, isEventCooldownActive, uiState } from "../store.svelte";
import { engineConnectionState, loadEngineConnection, markEngineUnavailable } from "./engineConnection.svelte";
import { getAppSetting } from "./settingsStore.svelte";
import { setVisibleInterval } from "./visibleInterval";
import { globalToast } from "./globalToast";
import { t } from "./i18n.svelte";
import { isRunningInTauri } from "./env";

const isTauri = isRunningInTauri();
const MAX_REFETCH_RETRIES = 10;
interface PollerRun {
  closed: boolean;
  revision: number;
  fullRefreshRevision: number;
  retries: number;
  stops: Array<() => void>;
  timers: Set<ReturnType<typeof setTimeout>>;
  source: EventSource | null;
  polling: (() => void) | null;
  fetching: { revision: number; promise: Promise<void> } | null;
}
let current: PollerRun | null = null;
let stopCurrent: (() => void) | null = null;
function owns(run: PollerRun, revision = run.revision): boolean {
  return current === run && !run.closed && run.revision === revision;
}
function canPublishResources(run: PollerRun, revision: number, generation: number | null): boolean {
  if (!owns(run, revision)) return false;
  if (!isTauri || !isDockerOnlyHost()) return true;
  const active = engineConnectionState.snapshot?.active;
  return !!active?.ready && generation !== null && generation === active.generation;
}
function later(run: PollerRun, callback: () => void, ms: number) {
  if (!owns(run)) return;
  const timer = setTimeout(() => { run.timers.delete(timer); if (owns(run)) callback(); }, ms);
  run.timers.add(timer);
}

export async function refreshManual() {
  const run = current;
  if (!run || !owns(run)) return;
  const [instanceList, sysInfo] = await Promise.all([
    supportsHostVms() ? colimaApi.listInstances().catch(() => [] as ColimaInstance[]) : Promise.resolve([]),
    sysMethods.checkSystem().catch(() => null),
  ]);
  if (!owns(run)) return;
  dashboardState.colimaInstances = instanceList;
  dashboardState.instancesLoaded = true;
  if (sysInfo) dashboardState.systemInfo = sysInfo;
}

function clearResources() {
  dockerState.originGeneration = null;
  resourceState.loadedGeneration = null;
  dockerState.containers = [];
  dockerState.images = [];
  resourceState.volumes = [];
  resourceState.networks = [];
  dashboardState.engineResources = null;
  dashboardState.composeProjectsCount = 0;
  uiState.focusResource = null;
  dockerState.loading = false;
  resourceState.volumesLoading = false;
  resourceState.networksLoading = false;
}

function handleConnectionLost(run: PollerRun) {
  if (!owns(run)) return;
  run.revision++;
  markEngineUnavailable();
  clearResources();
  // Existing hosts wait for a successful read after loss; cleared is not empty.
  const awaitingRecovery = !isTauri || !isDockerOnlyHost();
  dockerState.loading = awaitingRecovery;
  resourceState.volumesLoading = awaitingRecovery;
  resourceState.networksLoading = awaitingRecovery;
}

function scheduleRefetchRetry(run: PollerRun, revision: number) {
  if (!owns(run, revision)) return;
  if (++run.retries < MAX_REFETCH_RETRIES) later(run, () => { void refetchAllResources(); }, 5000);
  else console.warn(`[ColimaUI] refetchAllResources gave up after ${MAX_REFETCH_RETRIES} attempts`);
}

export async function refetchAllResources(): Promise<void> {
  const run = current;
  if (!run || !owns(run)) return;
  if (run.fetching?.revision === run.revision) return run.fetching.promise;
  const revision = run.revision;
  let expectedGeneration: number | null = null;
  const promise = (async () => {
    if (!isTauri || !isDockerOnlyHost()) {
      // Existing VM hosts and browser clients publish independent groups. A
      // Docker read failure must leave VM recovery and successful peers usable.
      const results = await Promise.allSettled([
        Promise.all([dockerApi.listContainers(true), dockerApi.listImages()]).then(([containers, images]) => {
          if (!owns(run, revision)) return;
          dockerState.containers = containers;
          dockerState.images = images;
          dockerState.loading = false;
        }),
        volumesApi.listVolumes().then((volumes) => {
          if (!owns(run, revision)) return;
          resourceState.volumes = volumes;
          resourceState.volumesLoading = false;
        }),
        networksApi.listNetworks().then((networks) => {
          if (!owns(run, revision)) return;
          resourceState.networks = networks;
          resourceState.networksLoading = false;
        }),
      ]);
      if (!owns(run, revision)) return;
      if (results.some(result => result.status === "rejected")) scheduleRefetchRetry(run, revision);
      else run.retries = 0;
      return;
    }
    if (isTauri && isDockerOnlyHost()) {
      if (!engineConnectionState.snapshot?.active.ready) {
        dockerState.loading = true;
        resourceState.volumesLoading = true;
        resourceState.networksLoading = true;
      }
      await loadEngineConnection();
      if (!owns(run, revision)) return;
      if (!engineConnectionState.snapshot?.active.ready) { handleConnectionLost(run); return; }
      expectedGeneration = engineConnectionState.snapshot.active.generation;
    }
    try {
      const [containers, images, volumes, networks] = await Promise.all([
        dockerApi.listContainers(true), dockerApi.listImages(), volumesApi.listVolumes(), networksApi.listNetworks(),
      ]);
      if (!canPublishResources(run, revision, expectedGeneration)) return;
      run.fullRefreshRevision++;
      dockerState.originGeneration = expectedGeneration;
      resourceState.loadedGeneration = expectedGeneration;
      dockerState.containers = containers;
      dockerState.images = images;
      resourceState.volumes = volumes;
      resourceState.networks = networks;
      dockerState.loading = false;
      dockerState.error = null;
      resourceState.volumesLoading = false;
      resourceState.networksLoading = false;
      run.retries = 0;
    } catch {
      if (!canPublishResources(run, revision, expectedGeneration)) return;
      run.fullRefreshRevision++;
      clearResources();
      dockerState.error = t("connection.resource_failed");
      scheduleRefetchRetry(run, revision);
    }
  })();
  run.fetching = { revision, promise };
  try { await promise; }
  finally {
    if (run.fetching?.promise === promise) run.fetching = null;
    if (owns(run, revision) && isTauri && isDockerOnlyHost() && engineConnectionState.snapshot?.active.ready && expectedGeneration !== engineConnectionState.snapshot.active.generation) void refetchAllResources();
  }
}

interface DockerSnapshot { containers?: unknown[]; images?: unknown[]; origin_generation?: number | null }
function handleDockerStateUpdated(run: PollerRun, data: DockerSnapshot) {
  if (!owns(run) || isEventCooldownActive()) return;
  if (isTauri && isDockerOnlyHost()) {
    const active = engineConnectionState.snapshot?.active;
    if (!active?.ready || active.generation == null || data.origin_generation !== active.generation) return;
  }
  dockerState.originGeneration = data.origin_generation ?? null;
  dockerState.containers = (data.containers || []).map((c) => normalizeContainer(c as Record<string, unknown>));
  dockerState.images = (data.images || []).map((i) => normalizeImage(i as Record<string, unknown>));
  dockerState.loading = false;
  // A C/I snapshot cannot certify recovery of the aggregate four-family read.
  if (!isTauri || !isDockerOnlyHost()) dockerState.error = null;
  const revision = run.revision;
  const fullRefreshRevision = run.fullRefreshRevision;
  const mayPublishRemainingResources = () => canPublishResources(run, revision, data.origin_generation ?? null)
    && (!isTauri || !isDockerOnlyHost() || run.fullRefreshRevision === fullRefreshRevision);
  later(run, () => {
    if (!mayPublishRemainingResources()) return;
    if (!isTauri || !isDockerOnlyHost()) {
      void volumesApi.listVolumes().then((volumes) => {
        if (!mayPublishRemainingResources()) return;
        resourceState.volumes = volumes;
        resourceState.volumesLoading = false;
      }).catch(() => {});
      void networksApi.listNetworks().then((networks) => {
        if (!mayPublishRemainingResources()) return;
        resourceState.networks = networks;
        resourceState.networksLoading = false;
      }).catch(() => {});
      return;
    }
    void Promise.all([volumesApi.listVolumes(), networksApi.listNetworks()]).then(([volumes, networks]) => {
      if (!mayPublishRemainingResources()) return;
      resourceState.volumes = volumes;
      resourceState.networks = networks;
      resourceState.volumesLoading = false;
      resourceState.networksLoading = false;
    }).catch(() => {
      if (isTauri && isDockerOnlyHost() && mayPublishRemainingResources()) dockerState.error = t("connection.resource_failed");
    });
  }, 1000);
}

function showUnsupportedRuntimeToast(payload: { runtime?: string; profiles?: string[] }) {
  globalToast("error", t("docker.unsupported_runtime", {
    default: "{runtime} runtime is not supported", runtime: payload.runtime || "containerd", profiles: payload.profiles?.join(", ") || "unknown",
  }), { hint: isDockerOnlyHost() ? t("connection.start_engine") : t("docker.unsupported_runtime_hint", {
    default: "ColimaUI requires the Docker runtime. Stop the profile and run: colima start --runtime docker",
  }) });
}

export function startDataPoller() {
  stopCurrent?.();
  const run: PollerRun = { closed: false, revision: 0, fullRefreshRevision: 0, retries: 0, stops: [], timers: new Set(), source: null, polling: null, fetching: null };
  current = run;
  const lost = () => handleConnectionLost(run);
  const recover = () => {
    if (!owns(run)) return;
    void loadCapabilities();
    void refetchAllResources();
  };
  const instances = (data: { instances?: ColimaInstance[] }) => {
    if (!owns(run) || !supportsHostVms()) return;
    if (data.instances) dashboardState.colimaInstances = data.instances;
    dashboardState.instancesLoaded = true;
    void loadCapabilities();
  };

  // Resolve host policy before acquiring host-specific work; unknown is not macOS.
  void (async () => {
    await loadCapabilities();
    if (!owns(run)) return;
    void refreshManual();
    void refetchAllResources();
    if (supportsHostVms()) {
      const enabled = getAppSetting("colimaui_auto_pause") === "true";
      const minutes = parseInt(getAppSetting("colimaui_auto_pause_mins") || "15", 10);
      void sysMethods.setResourceSaver(enabled, minutes).catch(() => {});
    }
    run.stops.push(setVisibleInterval(() => { void refreshManual(); }, 30000));
  })();

  if (isTauri) {
    void import("@tauri-apps/api/event").then(async (mod) => {
      const handlers: Array<[string, (event: { payload: unknown }) => void]> = [
        ["instances-update", (e) => instances(e.payload as { instances?: ColimaInstance[] })],
        ["docker-state-updated", (e) => handleDockerStateUpdated(run, e.payload as DockerSnapshot)],
        ["docker-connection-lost", lost], ["docker-reconnected", recover],
        ["docker-unsupported-runtime", (e) => showUnsupportedRuntimeToast(e.payload as { runtime?: string; profiles?: string[] })],
        ["navigate", (e) => { const page = (e.payload as { page?: unknown })?.page; if (typeof page === "string") uiState.currentPage = pageAvailable(page) ? page : "containers"; }],
        ["shutdown-blocked", () => { uiState.globalError = t("connection.shutdown_blocked"); }],
      ];
      for (const [name, handler] of handlers) {
        if (!owns(run)) break;
        const unlisten = await mod.listen(name, (e) => { if (owns(run)) handler(e); });
        if (owns(run)) run.stops.push(unlisten); else unlisten();
      }
    }).catch(() => { if (owns(run)) uiState.globalError = t("connection.live_failed"); });
  } else {
    let retryDelay = 2000;
    const failed = () => {
      run.source?.close(); run.source = null;
      if (!owns(run)) return;
      if (!run.polling) run.polling = setVisibleInterval(() => { void refreshManual(); void refetchAllResources(); }, 5000);
      later(run, () => { void connect(); }, retryDelay);
      retryDelay = Math.min(retryDelay * 2, 30000);
    };
    async function connect() {
      run.source?.close(); run.source = null;
      if (!owns(run)) return;
      try {
        const token = await getApiToken();
        const base = await resolveApiBase();
        if (!owns(run)) return;
        const es = new EventSource(token ? `${base}/api/events?token=${token}` : `${base}/api/events`);
        run.source = es;
        const jsonHandler = <T,>(callback: (data: T) => void) => (e: MessageEvent) => {
          if (!owns(run)) return;
          try { callback(JSON.parse(e.data) as T); } catch { /* Next snapshot retries malformed frames. */ }
        };
        es.addEventListener("instances-update", jsonHandler(instances));
        es.addEventListener("docker-state-updated", jsonHandler((data: DockerSnapshot) => handleDockerStateUpdated(run, data)));
        es.addEventListener("docker-connection-lost", lost);
        es.addEventListener("docker-reconnected", recover);
        es.addEventListener("docker-unsupported-runtime", jsonHandler(showUnsupportedRuntimeToast));
        es.onopen = () => {
          if (!owns(run)) return;
          retryDelay = 2000;
          run.polling?.(); run.polling = null;
        };
        es.onerror = failed;
      } catch { failed(); }
    }
    void connect();
  }
  const stop = () => {
    if (run.closed) return;
    run.closed = true;
    run.stops.forEach((close) => close());
    run.timers.forEach(clearTimeout); run.timers.clear();
    run.polling?.(); run.source?.close();
    if (current === run) { current = null; stopCurrent = null; }
  };
  stopCurrent = stop;
  return stop;
}
