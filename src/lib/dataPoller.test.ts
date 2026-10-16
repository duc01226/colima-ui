/**
 * Teardown contract for the data poller.
 *
 * Every bug this file guards against had the same shape: work scheduled before
 * a teardown ran anyway afterwards, against a context that no longer existed.
 * They are covered here because the module owns process-wide resources — an
 * interval, a retry timeout and an EventSource — and nothing else can observe
 * whether they were released.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const host = vi.hoisted(() => ({ native: false, windows: false, generation: 1, ready: true, listen: vi.fn(), handlers: new Map<string, (e: { payload: unknown }) => void>() }));
const listContainers = vi.fn();
const listImages = vi.fn();
const listVolumes = vi.fn();
const listNetworks = vi.fn();

/** Instances handed out by the module under test, so a test can assert on them. */
const eventSources: FakeEventSource[] = [];

class FakeEventSource {
  url: string;
  closed = false;
  listeners = new Map<string, (e: MessageEvent) => void>();
  onerror: ((e: unknown) => void) | null = null;
  constructor(url: string) {
    this.url = url;
    eventSources.push(this);
  }
  addEventListener(type: string, fn: (e: MessageEvent) => void) {
    this.listeners.set(type, fn);
  }
  close() {
    this.closed = true;
  }
}

vi.mock("./api", () => ({
  colimaApi: { listInstances: vi.fn().mockResolvedValue([]) },
  dockerApi: {
    listContainers: (...a: unknown[]) => listContainers(...a),
    listImages: (...a: unknown[]) => listImages(...a),
  },
  volumesApi: { listVolumes: (...a: unknown[]) => listVolumes(...a) },
  networksApi: { listNetworks: (...a: unknown[]) => listNetworks(...a) },
  sysMethods: {
    checkSystem: vi.fn().mockResolvedValue({}),
    setResourceSaver: vi.fn().mockResolvedValue(undefined),
  },
  getApiToken: vi.fn().mockResolvedValue("t"),
}));
vi.mock("./api/client", () => ({ resolveApiBase: vi.fn().mockResolvedValue("http://x") }));
vi.mock("../store/capabilities.svelte", () => ({ loadCapabilities: vi.fn().mockResolvedValue(undefined), isDockerOnlyHost: () => host.windows, supportsHostVms: () => !host.windows, pageAvailable: (page: string) => !host.windows || page !== "instances" }));
vi.mock("./engineConnection.svelte", () => ({
  engineConnectionState: { snapshot: { active: { get ready() { return host.ready; }, get generation() { return host.generation; } } } },
  loadEngineConnection: vi.fn().mockResolvedValue(undefined), markEngineUnavailable: () => { host.ready = false; },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: host.listen }));
vi.mock("./normalizers", () => ({ normalizeContainer: (c: unknown) => c, normalizeImage: (i: unknown) => i }));
vi.mock("./settingsStore.svelte", () => ({ getAppSetting: () => "" }));
// Browser mode: the SSE branch is the one that owns an EventSource.
vi.mock("./env", () => ({ isRunningInTauri: () => host.native }));
vi.mock("../store.svelte", () => ({
  dockerState: { containers: [], images: [], loading: true, error: null, originGeneration: null },
  resourceState: { loadedGeneration: null, volumes: [], networks: [], volumesLoading: true, networksLoading: true },
  dashboardState: { colimaInstances: [], systemInfo: {}, instancesLoaded: false },
  uiState: { currentPage: "dashboard" },
  isEventCooldownActive: () => false,
}));
vi.mock("./visibleInterval", () => ({ setVisibleInterval: () => () => {} }));

beforeEach(async () => {
  vi.resetModules();
  // Manual mocks survive resetModules; do not inherit an earlier read outcome.
  const { dockerState, resourceState } = await import("../store.svelte");
  Object.assign(dockerState, { containers: [], images: [], loading: true, error: null, originGeneration: null });
  Object.assign(resourceState, { loadedGeneration: null, volumes: [], networks: [], volumesLoading: true, networksLoading: true });
  for (const read of [listContainers, listImages, listVolumes, listNetworks]) read.mockReset();
  host.native = false; host.windows = false; host.ready = true; host.generation = 1; host.handlers.clear(); host.listen.mockReset();
  host.listen.mockImplementation(async (name: string, handler: (e: { payload: unknown }) => void) => { host.handlers.set(name, handler); return vi.fn(); });
  vi.useFakeTimers();
  eventSources.length = 0;
  vi.stubGlobal("EventSource", FakeEventSource as unknown as typeof EventSource);
  listContainers.mockResolvedValue([]);
  listImages.mockResolvedValue([]);
  listVolumes.mockResolvedValue([]);
  listNetworks.mockResolvedValue([]);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("teardown", () => {
  it("stops refetching once torn down", async () => {
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const stop = startDataPoller();
    await vi.runOnlyPendingTimersAsync();
    const before = listContainers.mock.calls.length;

    stop();
    await refetchAllResources();

    // A call that lands after teardown must not reach the network at all;
    // it used to keep polling a context that was already gone.
    expect(listContainers.mock.calls.length).toBe(before);
  });

  it("closes the EventSource so the connection is not leaked", async () => {
    const { startDataPoller } = await import("./dataPoller");
    const stop = startDataPoller();
    await vi.runOnlyPendingTimersAsync();
    expect(eventSources.length).toBeGreaterThan(0);

    stop();

    // Chrome allows six concurrent connections per origin; a leak here made
    // every later request to the API block, including the polling fallback.
    expect(eventSources.every((es) => es.closed)).toBe(true);
  });

  it("clears the retry timeout a failed refetch scheduled", async () => {
    listVolumes.mockRejectedValue(new Error("down"));
    const { startDataPoller } = await import("./dataPoller");
    const stop = startDataPoller();
    await vi.runOnlyPendingTimersAsync();

    // The failure must actually have armed a retry, or this proves nothing.
    expect(vi.getTimerCount()).toBeGreaterThan(0);

    stop();

    // The timer itself is asserted on, not its network effect: the guard at the
    // top of refetchAllResources would swallow the effect and let an orphaned
    // timeout — the original bug — go unnoticed.
    expect(vi.getTimerCount()).toBe(0);
  });

  it("a restart after teardown polls again instead of inheriting disposed", async () => {
    const { startDataPoller } = await import("./dataPoller");
    startDataPoller()();
    const after = listContainers.mock.calls.length;

    const stop2 = startDataPoller();
    await vi.runOnlyPendingTimersAsync();

    // HMR and a re-mounted root both take this path.
    expect(listContainers.mock.calls.length).toBeGreaterThan(after);
    stop2();
  });
});

// TC007/019/035: late native resources cannot outlive the owning app/availability.
describe("native run and origin fencing", () => {
  it("releases a native listener acquired after teardown without acquiring more", async () => {
    host.native = true; host.windows = true;
    let resolve!: (unlisten: () => void) => void;
    host.listen.mockReturnValueOnce(new Promise<() => void>((done) => { resolve = done; }));
    const { startDataPoller } = await import("./dataPoller");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    expect(host.listen).toHaveBeenCalledTimes(1);
    stop(); const unlisten = vi.fn(); resolve(unlisten); await vi.advanceTimersByTimeAsync(0);
    expect(unlisten).toHaveBeenCalledTimes(1); expect(host.listen).toHaveBeenCalledTimes(1);
  });
  it("drops an in-flight read after engine loss and rejects stale snapshot generations", async () => {
    host.native = true; host.windows = true;
    let resolve!: (items: unknown[]) => void;
    listContainers.mockReturnValueOnce(new Promise<unknown[]>((done) => { resolve = done; }));
    const { startDataPoller } = await import("./dataPoller");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    host.handlers.get("docker-connection-lost")!({ payload: {} });
    resolve([{ Id: "stale-read" }]); await vi.advanceTimersByTimeAsync(0);
    const { dockerState } = await import("../store.svelte"); expect(dockerState.containers).toEqual([]);
    host.ready = true; host.generation = 3;
    const snapshot = host.handlers.get("docker-state-updated")!;
    snapshot({ payload: { containers: [{ Id: "stale-event" }], images: [], origin_generation: 1 } });
    expect(dockerState.containers).toEqual([]);
    snapshot({ payload: { containers: [{ Id: "original-recovered" }], images: [], origin_generation: 3 } });
    expect(dockerState.containers).toEqual([{ Id: "original-recovered" }]);
    stop(); expect(vi.getTimerCount()).toBe(0);
  });
  it("drops a pre-recheck generation and loads the current original before publishing", async () => {
    host.native = true; host.windows = true;
    let resolve!: (items: unknown[]) => void;
    listContainers.mockReturnValueOnce(new Promise<unknown[]>((done) => { resolve = done; }));
    const { startDataPoller } = await import("./dataPoller");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    host.generation = 2;
    listContainers.mockResolvedValueOnce([{ Id: "rechecked-original" }]);
    resolve([{ Id: "before-recheck" }]); await vi.advanceTimersByTimeAsync(0);
    const { dockerState } = await import("../store.svelte");
    expect(dockerState.containers).toEqual([{ Id: "rechecked-original" }]);
    expect(dockerState.originGeneration).toBe(2);
    stop();
  });
  it("does not let a disposed run overwrite a restarted run", async () => {
    let resolve!: (items: unknown[]) => void;
    listContainers.mockReturnValueOnce(new Promise<unknown[]>((done) => { resolve = done; }));
    const { startDataPoller } = await import("./dataPoller");
    const first = startDataPoller(); await vi.advanceTimersByTimeAsync(0); first();
    listContainers.mockResolvedValueOnce([{ Id: "current" }]); const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    resolve([{ Id: "previous" }]); await vi.advanceTimersByTimeAsync(0);
    const { dockerState } = await import("../store.svelte"); expect(dockerState.containers).toEqual([{ Id: "current" }]); stop();
  });
  it("never clears a current snapshot when a pre-recheck read rejects", async () => {
    host.native = true; host.windows = true;
    let reject!: (error: Error) => void; let finish!: (items: unknown[]) => void;
    listContainers.mockReturnValueOnce(new Promise<unknown[]>((_, no) => { reject = no; }));
    listContainers.mockReturnValueOnce(new Promise<unknown[]>((yes) => { finish = yes; }));
    const { startDataPoller } = await import("./dataPoller");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    const { dockerState } = await import("../store.svelte");
    host.generation = 2;
    host.handlers.get("docker-state-updated")!({ payload: { containers: [{ Id: "current-original" }], images: [], origin_generation: 2 } });
    reject(new Error("old-generation refusal")); await vi.advanceTimersByTimeAsync(0);
    expect(listContainers).toHaveBeenCalledTimes(2);
    expect(dockerState.containers).toEqual([{ Id: "current-original" }]);
    expect(dockerState.originGeneration).toBe(2); expect(dockerState.error).toBeNull();
    stop(); finish([]); await vi.advanceTimersByTimeAsync(0);
    expect(dockerState.containers).toEqual([{ Id: "current-original" }]);
  });
  // TestSpec=TC-WINUI-002; TestSpec=TC-WINUI-007; TestSpec=TC-WINUI-035
  // BR06 / TC002/007/035: C/I push is partial; it cannot certify recovery of
  // the four-family state admitted by App, even in the same generation.
  it.each(["volumes", "networks"].flatMap(family => ["resolve", "reject"].map(outcome => ({ family, outcome }))))("retains a complete-read failure through partial recovery: %j", async ({ family, outcome }) => {
    host.native = true; host.windows = true;
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    const failedRead = family === "volumes" ? listVolumes : listNetworks;
    failedRead.mockRejectedValueOnce(new Error("current full-read failure"));
    await refetchAllResources();
    const { dockerState, resourceState } = await import("../store.svelte");
    const error = dockerState.error; expect(error).toBeTruthy();
    let resolve!: (rows: unknown[]) => void, reject!: (error: Error) => void;
    failedRead.mockReturnValueOnce(new Promise<unknown[]>((yes, no) => { resolve = yes; reject = no; }));
    host.handlers.get("docker-state-updated")!({ payload: { containers: [{ Id: "current-container" }], images: [], origin_generation: 1 } });
    expect(dockerState.error).toBe(error);
    expect(dockerState.containers).toEqual([{ Id: "current-container" }]);
    await vi.advanceTimersByTimeAsync(1000);
    expect(dockerState.error).toBe(error);
    if (outcome === "resolve") resolve([]); else reject(new Error("current partial failure"));
    await vi.advanceTimersByTimeAsync(0);
    expect(dockerState.error).toBe(error);
    for (const read of [listContainers, listImages, listVolumes, listNetworks]) read.mockResolvedValue([]);
    await refetchAllResources();
    expect(dockerState.error).toBeNull(); expect(dockerState.originGeneration).toBe(1);
    expect(resourceState.volumes).toEqual([]); expect(resourceState.networks).toEqual([]);
    stop();
  });

  it.each([true, false])("attributes a current event-side failure only under the native Windows policy (windows=%s)", async (windows) => {
    host.native = true; host.windows = windows;
    const { startDataPoller } = await import("./dataPoller");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    listNetworks.mockRejectedValueOnce(new Error("current event-side failure"));
    host.handlers.get("docker-state-updated")!({ payload: { containers: [], images: [], origin_generation: 1 } });
    await vi.advanceTimersByTimeAsync(1000);
    const { dockerState } = await import("../store.svelte");
    if (windows) expect(dockerState.error).toBeTruthy(); else expect(dockerState.error).toBeNull();
    stop();
  });

  it.each(["success", "failure"].flatMap(full => ["resolve", "reject"].map(partial => ({ full, partial }))))("fences event-side settlement after a newer full outcome in the same generation: %j", async ({ full, partial }) => {
    host.native = true; host.windows = true;
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    let resolve!: (rows: unknown[]) => void, reject!: (error: Error) => void;
    listVolumes.mockReturnValueOnce(new Promise<unknown[]>((yes, no) => { resolve = yes; reject = no; }));
    host.handlers.get("docker-state-updated")!({ payload: { containers: [], images: [], origin_generation: 1 } });
    await vi.advanceTimersByTimeAsync(1000);
    listVolumes.mockResolvedValue([{ Name: "new-full-volume" }]);
    if (full === "failure") listNetworks.mockRejectedValueOnce(new Error("new full failure"));
    await refetchAllResources();
    const { dockerState, resourceState } = await import("../store.svelte");
    const error = dockerState.error;
    if (full === "success") expect(error).toBeNull(); else expect(error).toBeTruthy();
    if (partial === "resolve") resolve([{ Name: "obsolete-pair-volume" }]); else reject(new Error("obsolete pair failure"));
    await vi.advanceTimersByTimeAsync(0);
    expect(dockerState.error).toBe(error);
    expect(resourceState.volumes).toEqual(full === "success" ? [{ Name: "new-full-volume" }] : []);
    stop();
  });

  it("does not acquire an event-side pair after a newer full refresh already settled", async () => {
    host.native = true; host.windows = true;
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    host.handlers.get("docker-state-updated")!({ payload: { containers: [], images: [], origin_generation: 1 } });
    await refetchAllResources();
    const calls = listVolumes.mock.calls.length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(listVolumes).toHaveBeenCalledTimes(calls); expect(listNetworks).toHaveBeenCalledTimes(calls);
    stop();
  });

  it("drops old event-side volume and network results after a generation-only recheck", async () => {
    host.native = true; host.windows = true;
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    let volumes!: (rows: unknown[]) => void; let networks!: (rows: unknown[]) => void;
    listVolumes.mockReturnValueOnce(new Promise<unknown[]>((yes) => { volumes = yes; }));
    listNetworks.mockReturnValueOnce(new Promise<unknown[]>((yes) => { networks = yes; }));
    host.handlers.get("docker-state-updated")!({ payload: { containers: [], images: [], origin_generation: 1 } });
    await vi.advanceTimersByTimeAsync(1000);
    expect(listVolumes).toHaveBeenCalledTimes(2); expect(listNetworks).toHaveBeenCalledTimes(2);
    host.generation = 2;
    listVolumes.mockResolvedValue([{ Name: "current-volume" }]); listNetworks.mockResolvedValue([{ Id: "current-network" }]);
    await refetchAllResources();
    volumes([{ Name: "old-volume" }]); networks([{ Id: "old-network" }]); await vi.advanceTimersByTimeAsync(0);
    const { dockerState, resourceState } = await import("../store.svelte");
    expect(resourceState.volumes).toEqual([{ Name: "current-volume" }]);
    expect(resourceState.networks).toEqual([{ Id: "current-network" }]);
    expect(dockerState.originGeneration).toBe(2); expect(dockerState.error).toBeNull();
    stop();
  });
  it("does not acquire obsolete event-side reads but still publishes current event-side reads", async () => {
    host.native = true; host.windows = true;
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    const snapshot = host.handlers.get("docker-state-updated")!;
    snapshot({ payload: { containers: [], images: [], origin_generation: 1 } });
    host.generation = 2; await refetchAllResources();
    const before = listVolumes.mock.calls.length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(listVolumes).toHaveBeenCalledTimes(before); expect(listNetworks).toHaveBeenCalledTimes(before);
    listVolumes.mockResolvedValue([{ Name: "live-volume" }]); listNetworks.mockResolvedValue([{ Id: "live-network" }]);
    snapshot({ payload: { containers: [], images: [], origin_generation: 2 } });
    await vi.advanceTimersByTimeAsync(1000);
    const { resourceState } = await import("../store.svelte");
    expect(listVolumes).toHaveBeenCalledTimes(before + 1); expect(listNetworks).toHaveBeenCalledTimes(before + 1);
    expect(resourceState.volumes).toEqual([{ Name: "live-volume" }]); expect(resourceState.networks).toEqual([{ Id: "live-network" }]);
    stop();
  });
});

// TestSpec=TC-WINUI-002; TestSpec=TC-WINUI-007; TestSpec=TC-WINUI-034; TestSpec=TC-WINUI-035
// Container provenance is useful independently; it cannot certify that all
// four resource families have completed for the current engine generation.
describe("complete resource admission", () => {
  it.each(["cold", "loss", "recheck"])("waits for the full current read despite current partial data (%s)", async (transition) => {
    host.native = true; host.windows = true;
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const { dockerState, resourceState } = await import("../store.svelte");
    let finish!: (rows: unknown[]) => void;
    const held = new Promise<unknown[]>((yes) => { finish = yes; });
    if (transition === "cold") listVolumes.mockReturnValueOnce(held);
    const stop = startDataPoller();
    try {
      await vi.advanceTimersByTimeAsync(0);
      if (transition !== "cold") {
        expect(resourceState.loadedGeneration).toBe(1);
        if (transition === "loss") {
          host.handlers.get("docker-connection-lost")!({ payload: {} });
          expect(resourceState.loadedGeneration).toBeNull();
        }
        host.ready = true; host.generation = 2;
        listVolumes.mockReturnValueOnce(held);
      }
      const pending = refetchAllResources(); await vi.advanceTimersByTimeAsync(0);
      const priorComplete = transition === "recheck" ? 1 : null;
      listVolumes.mockResolvedValue([{ Name: "partial-volume" }]);
      host.handlers.get("docker-state-updated")!({ payload: { containers: [{ Id: "current-container" }], images: [], origin_generation: host.generation } });
      expect(dockerState.originGeneration).toBe(host.generation);
      expect(dockerState.containers).toEqual([{ Id: "current-container" }]);
      expect(dockerState.error).toBeNull();
      expect(resourceState.loadedGeneration).toBe(priorComplete);
      await vi.advanceTimersByTimeAsync(1000);
      expect(resourceState.volumes).toEqual([{ Name: "partial-volume" }]);
      expect(resourceState.loadedGeneration).toBe(priorComplete);
      finish([]); await pending;
      expect(resourceState.loadedGeneration).toBe(host.generation);
      expect(resourceState.volumes).toEqual([]); expect(resourceState.networks).toEqual([]);
      // Once complete, ordinary live partial updates remain valid.
      host.handlers.get("docker-state-updated")!({ payload: { containers: [{ Id: "live-container" }], images: [], origin_generation: host.generation } });
      await vi.advanceTimersByTimeAsync(1000);
      expect(resourceState.loadedGeneration).toBe(host.generation);
      expect(dockerState.containers).toEqual([{ Id: "live-container" }]);
      expect(resourceState.volumes).toEqual([{ Name: "partial-volume" }]);
    } finally { stop(); finish([]); }
  });

  it("never certifies a complete snapshot from an already disposed run", async () => {
    host.native = true; host.windows = true;
    let finish!: (rows: unknown[]) => void;
    listVolumes.mockReturnValueOnce(new Promise<unknown[]>((yes) => { finish = yes; }));
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const { resourceState } = await import("../store.svelte");
    const stop = startDataPoller(); await vi.advanceTimersByTimeAsync(0);
    const pending = refetchAllResources(); stop(); finish([]); await pending;
    expect(resourceState.loadedGeneration).toBeNull();
  });
});

// TestSpec=TC-WINUI-020: existing VM hosts and browser clients retain useful
// successful groups and VM recovery when Docker or an unrelated family fails.
describe("supported-platform resource preservation", () => {
  const modes = ["native-posix", "browser"] as const;
  const seed = {
    containers: [{ Id: "old-container" }], images: [{ Id: "old-image" }],
    volumes: [{ Name: "old-volume" }], networks: [{ Id: "old-network" }],
  };
  const fresh = {
    containers: [{ Id: "fresh-container" }], images: [{ Id: "fresh-image" }],
    volumes: [{ Name: "fresh-volume" }], networks: [{ Id: "fresh-network" }],
  };
  function snapshot(mode: typeof modes[number]) {
    const data = { containers: [], images: [] };
    if (mode === "native-posix") host.handlers.get("docker-state-updated")!({ payload: data });
    else eventSources[eventSources.length - 1]!.listeners.get("docker-state-updated")!(new MessageEvent("docker-state-updated", { data: JSON.stringify(data) }));
  }
  it.each(modes.flatMap(mode => (["containers", "images", "volumes", "networks"] as const).map(family => ({ mode, family }))))("keeps successful groups when a peer full read fails ($mode/$family)", async ({ mode, family }) => {
    host.native = mode === "native-posix";
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const { dockerState, resourceState } = await import("../store.svelte");
    const stop = startDataPoller();
    try {
      await vi.advanceTimersByTimeAsync(0);
      Object.assign(dockerState, { containers: seed.containers, images: seed.images, loading: false });
      Object.assign(resourceState, { volumes: seed.volumes, networks: seed.networks, volumesLoading: false, networksLoading: false });
      const reads = { containers: listContainers, images: listImages, volumes: listVolumes, networks: listNetworks };
      for (const key of Object.keys(reads) as Array<keyof typeof reads>) reads[key].mockResolvedValueOnce(fresh[key]);
      reads[family].mockReset().mockRejectedValueOnce(new Error("owned peer unavailable")).mockResolvedValue(fresh[family]);
      await refetchAllResources();
      const ciFailed = family === "containers" || family === "images";
      expect(dockerState.containers).toEqual(ciFailed ? seed.containers : fresh.containers);
      expect(dockerState.images).toEqual(ciFailed ? seed.images : fresh.images);
      expect(resourceState.volumes).toEqual(family === "volumes" ? seed.volumes : fresh.volumes);
      expect(resourceState.networks).toEqual(family === "networks" ? seed.networks : fresh.networks);
      expect(dockerState.error).toBeNull();
      const calls = listContainers.mock.calls.length;
      await vi.advanceTimersByTimeAsync(5000);
      expect(listContainers).toHaveBeenCalledTimes(calls + 1);
      expect(dockerState.error).toBeNull();
    } finally { stop(); }
  });

  it.each(modes)("publishes a successful group before a held peer completes (%s)", async (mode) => {
    host.native = mode === "native-posix";
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const { dockerState, resourceState } = await import("../store.svelte");
    const stop = startDataPoller();
    let finish!: (rows: unknown[]) => void;
    try {
      await vi.advanceTimersByTimeAsync(0);
      listVolumes.mockReturnValueOnce(new Promise<unknown[]>(yes => { finish = yes; }));
      listContainers.mockResolvedValueOnce(fresh.containers); listImages.mockResolvedValueOnce(fresh.images); listNetworks.mockResolvedValueOnce(fresh.networks);
      const pending = refetchAllResources(); await vi.advanceTimersByTimeAsync(0);
      expect(dockerState.containers).toEqual(fresh.containers); expect(dockerState.images).toEqual(fresh.images);
      expect(resourceState.networks).toEqual(fresh.networks); expect(resourceState.volumes).toEqual([]);
      finish(fresh.volumes); await pending;
      expect(resourceState.volumes).toEqual(fresh.volumes); expect(dockerState.error).toBeNull();
    } finally { stop(); finish?.([]); }
  });

  it.each(modes.flatMap(mode => (["volumes", "networks"] as const).map(family => ({ mode, family }))))("keeps the successful delayed event peer ($mode/$family fails)", async ({ mode, family }) => {
    host.native = mode === "native-posix";
    const { startDataPoller } = await import("./dataPoller");
    const { dockerState, resourceState } = await import("../store.svelte");
    const stop = startDataPoller();
    try {
      await vi.advanceTimersByTimeAsync(0);
      Object.assign(resourceState, { volumes: seed.volumes, networks: seed.networks });
      listVolumes.mockResolvedValueOnce(fresh.volumes); listNetworks.mockResolvedValueOnce(fresh.networks);
      (family === "volumes" ? listVolumes : listNetworks).mockReset().mockRejectedValueOnce(new Error("event peer unavailable"));
      snapshot(mode); await vi.advanceTimersByTimeAsync(1000);
      expect(resourceState.volumes).toEqual(family === "volumes" ? seed.volumes : fresh.volumes);
      expect(resourceState.networks).toEqual(family === "networks" ? seed.networks : fresh.networks);
      expect(dockerState.error).toBeNull();
    } finally { stop(); }
  });

  const heldGroups = [
    { source: "full", family: "containers" }, { source: "full", family: "volumes" }, { source: "full", family: "networks" },
    { source: "event", family: "volumes" }, { source: "event", family: "networks" },
  ] as const;
  it.each(heldGroups.flatMap(({ source, family }) => (["loss", "dispose", "restart"] as const).map(end => ({ source, family, end }))))("refuses late POSIX group settlement after $end ($source/$family)", async ({ source, family, end }) => {
    host.native = true;
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const { dockerState, resourceState } = await import("../store.svelte");
    const stop = startDataPoller(); let replacementStop: (() => void) | undefined;
    let finish!: (rows: unknown[]) => void;
    try {
      await vi.advanceTimersByTimeAsync(0);
      ({ containers: listContainers, volumes: listVolumes, networks: listNetworks })[family].mockReturnValueOnce(new Promise<unknown[]>(yes => { finish = yes; }));
      let pending: Promise<void> | undefined;
      if (source === "full") { pending = refetchAllResources(); await vi.advanceTimersByTimeAsync(0); }
      else { snapshot("native-posix"); await vi.advanceTimersByTimeAsync(1000); }
      if (end === "loss") host.handlers.get("docker-connection-lost")!({ payload: {} });
      else if (end === "dispose") stop();
      else { replacementStop = startDataPoller(); await vi.advanceTimersByTimeAsync(0); }
      Object.assign(dockerState, { containers: fresh.containers, images: fresh.images });
      Object.assign(resourceState, { volumes: fresh.volumes, networks: fresh.networks });
      finish(seed[family]); if (pending) await pending; await vi.advanceTimersByTimeAsync(0);
      expect(dockerState.containers).toEqual(fresh.containers); expect(dockerState.images).toEqual(fresh.images);
      expect(resourceState.volumes).toEqual(fresh.volumes); expect(resourceState.networks).toEqual(fresh.networks); expect(dockerState.error).toBeNull();
      stop(); replacementStop?.(); expect(vi.getTimerCount()).toBe(0);
    } finally { stop(); replacementStop?.(); finish?.([]); }
  });

  it.each(["loss", "dispose", "restart"])("does not retry a late rejected POSIX refresh after %s", async (end) => {
    host.native = true;
    const { startDataPoller, refetchAllResources } = await import("./dataPoller");
    const stop = startDataPoller(); let replacementStop: (() => void) | undefined;
    let reject!: (error: Error) => void;
    try {
      await vi.advanceTimersByTimeAsync(0);
      listVolumes.mockReturnValueOnce(new Promise<unknown[]>((_, no) => { reject = no; }));
      const pending = refetchAllResources(); await vi.advanceTimersByTimeAsync(0);
      if (end === "loss") host.handlers.get("docker-connection-lost")!({ payload: {} });
      else if (end === "dispose") stop();
      else { replacementStop = startDataPoller(); await vi.advanceTimersByTimeAsync(0); }
      reject(new Error("old unavailable group")); await pending;
      const count = listContainers.mock.calls.length;
      await vi.advanceTimersByTimeAsync(5000);
      expect(listContainers).toHaveBeenCalledTimes(count);
      stop(); replacementStop?.(); expect(vi.getTimerCount()).toBe(0);
    } finally { stop(); replacementStop?.(); }
  });
});

// TestSpec=TC-WINUI-020: a stopped Docker daemon is a normal recovery state.
it.each([true, false])("retains all prior POSIX groups when every Docker read fails (native=%s)", async (native) => {
  host.native = native;
  const { startDataPoller, refetchAllResources } = await import("./dataPoller");
  const { dockerState, resourceState } = await import("../store.svelte");
  const old = { containers: [{ Id: "retained-container" }], images: [{ Id: "retained-image" }], volumes: [{ Name: "retained-volume" }], networks: [{ Id: "retained-network" }] };
  Object.assign(dockerState, { containers: old.containers, images: old.images, loading: false });
  Object.assign(resourceState, { volumes: old.volumes, networks: old.networks, volumesLoading: false, networksLoading: false });
  for (const read of [listContainers, listImages, listVolumes, listNetworks]) read.mockRejectedValue(new Error("Docker daemon stopped"));
  const stop = startDataPoller();
  try {
    await vi.advanceTimersByTimeAsync(0); await refetchAllResources();
    expect(dockerState.containers).toEqual(old.containers); expect(dockerState.images).toEqual(old.images);
    expect(resourceState.volumes).toEqual(old.volumes); expect(resourceState.networks).toEqual(old.networks);
    expect(dockerState.error).toBeNull(); expect(dockerState.loading).toBe(false);
  } finally { stop(); }
});

it("bounds repeated POSIX failure retries and cancels them on teardown", async () => {
  const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
  for (const read of [listContainers, listImages, listVolumes, listNetworks]) read.mockRejectedValue(new Error("still stopped"));
  const { startDataPoller } = await import("./dataPoller");
  const stop = startDataPoller();
  try {
    await vi.advanceTimersByTimeAsync(50000);
    for (const read of [listContainers, listImages, listVolumes, listNetworks]) expect(read).toHaveBeenCalledTimes(10);
    expect(warning).toHaveBeenCalledExactlyOnceWith("[ColimaUI] refetchAllResources gave up after 10 attempts");
    stop(); expect(vi.getTimerCount()).toBe(0);
  } finally { stop(); }
});

// TestSpec=TC-WINUI-020: legacy loss remains pending; native Windows keeps
// its unavailable gate. A partial C/I event cannot turn failed V/N into empty.
it.each(["native-posix", "browser-posix", "browser-windows", "native-windows"] as const)("preserves host loss loading until successful recovery (%s)", async mode => {
  host.native = mode.startsWith("native"); host.windows = mode.endsWith("windows");
  listContainers.mockResolvedValue([{ Id: "fixture-container" }]); listImages.mockResolvedValue([{ Id: "fixture-image" }]);
  listVolumes.mockResolvedValue([{ Name: "fixture-volume" }]); listNetworks.mockResolvedValue([{ Id: "fixture-network" }]);
  const { startDataPoller, refetchAllResources } = await import("./dataPoller");
  const { dockerState, resourceState } = await import("../store.svelte");
  const stop = startDataPoller();
  try {
    await vi.advanceTimersByTimeAsync(0); await refetchAllResources();
    expect(resourceState.volumesLoading).toBe(false); expect(resourceState.networksLoading).toBe(false);
    listVolumes.mockRejectedValue(new Error("unreachable volume read"));
    listNetworks.mockRejectedValue(new Error("unreachable network read"));
    const volumeReads = listVolumes.mock.calls.length, networkReads = listNetworks.mock.calls.length;
    const emit = (name: string, payload: unknown) => {
      if (host.native) host.handlers.get(name)!({ payload });
      else eventSources[eventSources.length - 1]!.listeners.get(name)!(new MessageEvent(name, { data: JSON.stringify(payload) }));
    };
    emit("docker-connection-lost", {});
    const legacyPending = mode !== "native-windows";
    expect(dockerState.loading).toBe(legacyPending);
    expect(resourceState.volumesLoading).toBe(legacyPending); expect(resourceState.networksLoading).toBe(legacyPending);
    expect(dockerState.containers).toEqual([]); expect(resourceState.volumes).toEqual([]); expect(resourceState.networks).toEqual([]);
    expect(resourceState.loadedGeneration).toBeNull();
    emit("docker-state-updated", { containers: [], images: [] });
    await vi.advanceTimersByTimeAsync(1000);
    expect(dockerState.loading).toBe(false);
    expect(resourceState.volumesLoading).toBe(legacyPending); expect(resourceState.networksLoading).toBe(legacyPending);
    expect(listVolumes).toHaveBeenCalledTimes(volumeReads + (legacyPending ? 1 : 0));
    expect(listNetworks).toHaveBeenCalledTimes(networkReads + (legacyPending ? 1 : 0));
    host.ready = true;
    listContainers.mockResolvedValue([]); listImages.mockResolvedValue([]); listVolumes.mockResolvedValue([]); listNetworks.mockResolvedValue([]);
    await refetchAllResources();
    expect(dockerState.loading).toBe(false); expect(dockerState.error).toBeNull();
    expect(resourceState.volumesLoading).toBe(false); expect(resourceState.networksLoading).toBe(false);
    expect(resourceState.volumes).toEqual([]); expect(resourceState.networks).toEqual([]);
  } finally { stop(); expect(vi.getTimerCount()).toBe(0); }
});
