/** TestSpec=TC-WINUI-020. Real App and VM views keep explicit VM recovery
 * reachable on existing hosts when Docker is stopped. Transport/effect seams
 * are substituted; this does not prove native VM or Windows qualification. */
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { tick } from "svelte";

const seam = vi.hoisted(() => ({
  containers: vi.fn(), images: vi.fn(), volumes: vi.fn(), networks: vi.fn(),
  instances: vi.fn(), vms: vi.fn(), startColima: vi.fn(), startLima: vi.fn(),
  connection: vi.fn(), save: vi.fn(), restart: vi.fn(), handlers: new Map<string, (e: { payload: unknown }) => void>(),
}));
vi.mock("./lib/api", () => ({
  dockerApi: { listContainers: seam.containers, listImages: seam.images },
  volumesApi: { listVolumes: seam.volumes }, networksApi: { listNetworks: seam.networks },
  colimaApi: { listInstances: seam.instances, startInstance: seam.startColima },
  limaApi: { list: seam.vms, start: seam.startLima },
  kindApi: { list: vi.fn().mockResolvedValue("") }, aiApi: {},
  sysMethods: { getRuntimeInfo: vi.fn().mockResolvedValue("docker"), checkSystem: vi.fn().mockResolvedValue({}), setResourceSaver: vi.fn().mockResolvedValue(undefined) },
  getApiToken: vi.fn().mockResolvedValue("fixture"),
}));
vi.mock("./lib/api/system", () => ({ connectionApi: { get: seam.connection, save: seam.save, restart: seam.restart } }));
vi.mock("./lib/settingsStore.svelte", async (original) => ({
  ...await original<typeof import("./lib/settingsStore.svelte")>(),
  loadAllSettings: vi.fn().mockResolvedValue(undefined), setAppSetting: vi.fn(),
  getAppSetting: (key: string, fallback = "") => ["colimaui_setup_complete", "colimaui_tour_complete"].includes(key) ? "true" : fallback,
}));
vi.mock("./store/capabilities.svelte", async (original) => ({
  ...await original<typeof import("./store/capabilities.svelte")>(), loadCapabilities: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("./lib/env", () => ({ isRunningInTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name: string, handler: (e: { payload: unknown }) => void) => {
  seam.handlers.set(name, handler); return () => { if (seam.handlers.get(name) === handler) seam.handlers.delete(name); };
}) }));
vi.mock("./lib/visibleInterval", () => ({ setVisibleInterval: () => () => {} }));
vi.mock("./lib/keyboardShortcuts", () => ({ handleGlobalKeydown: vi.fn() }));
vi.mock("./lib/transferNotifications", () => ({ startTransferNotifications: () => vi.fn() }));
vi.mock("./lib/announcements", () => ({ startAnnouncements: () => vi.fn() }));
vi.mock("./lib/crashReporter", () => ({ installCrashReporter: vi.fn() }));
vi.mock("./lib/globalToast", () => ({ globalToast: vi.fn() }));
vi.mock("./lib/presetStateManager", () => ({
  BUILT_IN_PRESETS: [], getCurrentPresetForInstance: () => null,
  handleInstanceStarted: vi.fn().mockResolvedValue(undefined), handleInstanceStopping: vi.fn(),
}));
// App admission, the real shared poller, stores and both real VM views run.
// No resource/VM-page replacement supplies a synthetic Start control.
vi.mock("./components/Sidebar.svelte", () => ({ default: () => {} }));
vi.mock("./components/AiChatPanel.svelte", () => ({ default: () => {} }));
vi.mock("./components/notifications/NotificationPanel.svelte", () => ({ default: () => {} }));
vi.mock("./components/ErrorDetailPanel.svelte", () => ({ default: () => {} }));
vi.mock("./components/ToastContainer.svelte", () => ({ default: () => {} }));
vi.mock("./components/ConfirmDialog.svelte", () => ({ default: () => {} }));
vi.mock("./components/SetupWizard.svelte", () => ({ default: () => {} }));
vi.mock("./components/GettingStartedTour.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Dashboard.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Containers.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Images.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Topology.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Activity.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Compose.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Security.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Models.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Kubernetes.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Settings.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Terminal.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Help.svelte", () => ({ default: () => {} }));

import App from "./App.svelte";
import { dockerState, dashboardState, uiState, resourceState } from "./store.svelte";
import { capabilitiesState } from "./store/capabilities.svelte";
import { engineConnectionState } from "./lib/engineConnection.svelte";
import { refetchAllResources } from "./lib/dataPoller";

const instance = { name: "colima-fixture", status: "Stopped", arch: "aarch64", cpus: 2, memory: 2, disk: 60, runtime: "docker", address: "", kubernetes: false };
const vm = { name: "lima-fixture", status: "Stopped", arch: "aarch64", cpus: 2, memory: "2GiB", disk: "60GiB" };
beforeEach(() => {
  vi.clearAllMocks(); localStorage.clear(); seam.handlers.clear();
  for (const fn of [seam.containers, seam.images, seam.volumes, seam.networks]) fn.mockReset().mockRejectedValue(new Error("Docker daemon is stopped"));
  seam.instances.mockReset().mockResolvedValue([instance]); seam.vms.mockReset().mockResolvedValue([vm]);
  seam.startColima.mockReset().mockResolvedValue(undefined); seam.startLima.mockReset().mockResolvedValue(undefined);
  capabilitiesState.platform = { os: "macos", arch: "aarch64", wsl: false, wsl_available: false, package_managers: [] };
  capabilitiesState.items = [{ id: "colima", name: "Colima", state: "installed_not_running" }, { id: "docker", name: "Docker", state: "installed_not_running" }];
  capabilitiesState.failed = false; capabilitiesState.loading = false;
  dockerState.containers = []; dockerState.images = []; dockerState.error = null; dockerState.loading = true; dockerState.originGeneration = null;
  resourceState.volumes = []; resourceState.networks = []; resourceState.loadedGeneration = null;
  dashboardState.colimaInstances = []; dashboardState.instancesLoaded = false;
  engineConnectionState.snapshot = null; engineConnectionState.loading = false;
  uiState.currentPage = "help"; uiState.globalError = null; uiState.focusResource = null;
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); });

it.each((["macos", "linux"] as const).flatMap(os => (["instances", "linux-vms"] as const).map(page => ({ os, page }))))("keeps actual $os $page Start reachable with Docker stopped", async ({ os, page }) => {
  capabilitiesState.platform!.os = os;
  render(App);
  await waitFor(() => expect(seam.handlers.has("docker-reconnected")).toBe(true));
  await refetchAllResources();
  expect(seam.containers).toHaveBeenCalled(); expect(seam.volumes).toHaveBeenCalled();
  expect(dockerState.error).toBeNull();
  // An error already retained from an earlier failed read must not authorize a
  // Windows-only shell gate on a known POSIX host either. This independently
  // protects App's policy even when the repaired poller no longer writes it.
  dockerState.error = "Earlier Docker resource read failed";
  uiState.currentPage = page; await tick();
  const start = await screen.findByRole("button", { name: /^Start$/ });
  expect(start).toBeEnabled();
  await fireEvent.click(start);
  if (page === "instances") {
    await waitFor(() => expect(seam.startColima).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ profile: "fixture", runtime: "docker" })));
    expect(seam.startLima).not.toHaveBeenCalled();
  } else {
    await waitFor(() => expect(seam.startLima).toHaveBeenCalledExactlyOnceWith("lima-fixture"));
    expect(seam.startColima).not.toHaveBeenCalled();
  }
  expect(seam.save).not.toHaveBeenCalled(); expect(seam.restart).not.toHaveBeenCalled();
  expect(uiState.globalError).toBeNull();
});

// TestSpec=TC-WINUI-020; BR06: an outage must not become a healthy empty list.
// These real pages are mounted and their initial reads settled before loss.
it.each((["macos", "linux"] as const).flatMap(os => (["volumes", "networks"] as const).map(page => ({ os, page }))))("keeps mounted $os $page pending through actual loss and failed reads", async ({ os, page }) => {
  vi.useFakeTimers();
  capabilitiesState.platform!.os = os;
  capabilitiesState.items = [{ id: "colima", name: "Colima", state: "running" }, { id: "docker", name: "Docker", state: "running" }];
  const volume = { Name: "fixture-volume", Driver: "local", Mountpoint: "/fixture", Scope: "local", Labels: "" };
  const network = { Id: "fixture-network-id", Name: "fixture-network", Driver: "bridge", Scope: "local", Ipv6: "false", Internal: "false", Labels: "" };
  seam.containers.mockResolvedValue([]); seam.images.mockResolvedValue([]);
  seam.volumes.mockResolvedValue([volume]); seam.networks.mockResolvedValue([network]);
  render(App);
  await vi.advanceTimersByTimeAsync(0); await vi.dynamicImportSettled();
  expect(seam.handlers.has("docker-reconnected")).toBe(true);
  await refetchAllResources();
  const read = page === "volumes" ? seam.volumes : seam.networks;
  let finishMount!: (rows: unknown[]) => void;
  const mountedRead = new Promise<unknown[]>(yes => { finishMount = yes; });
  read.mockReturnValueOnce(mountedRead);
  const beforeMount = read.mock.calls.length;
  uiState.currentPage = page; await tick();
  expect(read).toHaveBeenCalledTimes(beforeMount + 1);
  finishMount(page === "volumes" ? [volume] : [network]);
  await mountedRead; await tick();
  expect(screen.getByText(page === "volumes" ? volume.Name : network.Name)).toBeInTheDocument();
  expect(resourceState.volumesLoading).toBe(false); expect(resourceState.networksLoading).toBe(false);

  seam.volumes.mockRejectedValue(new Error("Docker daemon unreachable"));
  seam.networks.mockRejectedValue(new Error("Docker daemon unreachable"));
  const volumeReads = seam.volumes.mock.calls.length, networkReads = seam.networks.mock.calls.length;
  seam.handlers.get("docker-connection-lost")!({ payload: {} });
  seam.handlers.get("docker-state-updated")!({ payload: { containers: [], images: [] } });
  await tick();
  // Advance the production event debounce, not a guessed readiness delay.
  await vi.advanceTimersByTimeAsync(1000); await tick();
  expect(seam.volumes).toHaveBeenCalledTimes(volumeReads + 1);
  expect(seam.networks).toHaveBeenCalledTimes(networkReads + 1);
  expect(screen.getByText(page === "volumes" ? "Loading volumes..." : "Loading networks...")).toBeInTheDocument();
  expect(screen.queryByText(page === "volumes" ? "No volumes found" : "No networks found")).not.toBeInTheDocument();
  expect(resourceState.volumes).toEqual([]); expect(resourceState.networks).toEqual([]);
  expect(capabilitiesState.items.every(item => item.state === "running")).toBe(true);

  // Genuine successful empty recovery must still reveal the normal empty state.
  seam.volumes.mockResolvedValue([]); seam.networks.mockResolvedValue([]);
  await refetchAllResources(); await tick();
  expect(screen.getByText(page === "volumes" ? "No volumes found" : "No networks found")).toBeInTheDocument();
  expect(screen.queryByText(page === "volumes" ? "Loading volumes..." : "Loading networks...")).not.toBeInTheDocument();
  expect(resourceState.volumesLoading).toBe(false); expect(resourceState.networksLoading).toBe(false);
  expect(uiState.globalError).toBeNull();
});
