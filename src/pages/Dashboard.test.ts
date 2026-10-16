import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { tick } from "svelte";
import type { EngineResources, SystemInfo } from "../lib/api";
const seam = vi.hoisted(() => ({ engine: vi.fn(), compose: vi.fn(), k8s: vi.fn(), kind: vi.fn(), namespaces: vi.fn(), pods: vi.fn(), lima: vi.fn() }));
vi.mock("../lib/api", () => ({
  systemApi: { engineResources: seam.engine }, composeApi: { list: seam.compose },
  k8sApi: { check: seam.k8s, namespaces: seam.namespaces, pods: seam.pods }, kindApi: { list: seam.kind }, limaApi: { list: seam.lima },
}));
import Dashboard from "./Dashboard.svelte";
import { capabilitiesState } from "../store/capabilities.svelte";
import { dashboardState, dockerState, resourceState } from "../store.svelte";
import { engineConnectionState } from "../lib/engineConnection.svelte";

const engine: EngineResources = { available: true, engineName: "fixture-engine", serverVersion: "fixture", operatingSystem: "Linux", cpuCores: 8, cpuPercent: 12.5, memoryTotalBytes: 8 * 1073741824, memoryUsedBytes: 2 * 1073741824, diskUsedBytes: 3 * 1073741824, diskReclaimableBytes: 1073741824, containersRunning: 0 };
const systemInfo: SystemInfo = { docker_installed: true, docker_version: "fixture", colima_installed: true, colima_version: "fixture", lima_installed: true, lima_version: "fixture" };
beforeEach(() => {
  vi.clearAllMocks(); seam.engine.mockResolvedValue(engine); seam.compose.mockResolvedValue([]);
  seam.k8s.mockResolvedValue("ready"); seam.kind.mockResolvedValue("fixture-kind\n"); seam.namespaces.mockResolvedValue([]); seam.pods.mockResolvedValue([]); seam.lima.mockResolvedValue([]);
  capabilitiesState.platform = null;
  dashboardState.lastFetch = 0; dashboardState.engineResources = null; dashboardState.composeProjectsCount = 0;
  dashboardState.colimaInstances = [{ name: "stale-vm", status: "Running", arch: "aarch64", cpus: 4, memory: 1073741824, disk: 2 * 1073741824, runtime: "docker", address: "", kubernetes: true }];
  dashboardState.k8sStatus = { connected: true, pods: 7, namespaces: 2, kindClusters: 1 }; dashboardState.linuxVMs = [];
  dockerState.containers = []; dockerState.images = []; resourceState.volumes = []; resourceState.networks = [];
  dockerState.originGeneration = 1;
  engineConnectionState.snapshot = { active: { context: "fixture", endpoint: "npipe:////./pipe/fixture", engine_id: "fixture-engine", generation: 1, ready: true, error: null }, pending: null, contexts: [], discovery_error: null };
});
afterEach(cleanup);
function host(os: "windows" | "macos" | "linux" | "unknown") {
  capabilitiesState.platform = os === "unknown" ? null : { os, arch: "x86_64", wsl: false, wsl_available: false, package_managers: [] };
}
function unsupportedAbsent() {
  for (const name of [/running vms/i, /stopped vms/i, /kubernetes/i, /kind clusters/i, /linux vms/i, /create instance/i, /view all/i]) expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
  for (const text of ["stale-vm", "Colima", "Lima", "VM allocation (engine unreachable)"]) expect(screen.queryByText(text)).not.toBeInTheDocument();
  for (const read of [seam.k8s, seam.kind, seam.namespaces, seam.pods, seam.lima]) expect(read).not.toHaveBeenCalled();
}

// TestSpec=TC-WINUI-015; TestSpec=TC-WINUI-018 — partial Dashboard contract.
// BR06 / TC015/018: actual component + actual central host policy. Backend
// refusals are separate; the Dashboard must not acquire or advertise them.
it.each(["windows", "unknown"] as const)("keeps unsupported families out of the actual %s Dashboard", async (os) => {
  host(os); const navigate = vi.fn(); render(Dashboard, { systemInfo, loading: false, onNavigate: navigate });
  if (os === "windows") {
    await waitFor(() => expect(screen.getByText(/fixture-engine/)).toBeInTheDocument());
    expect(screen.getByText("12.5%")).toBeInTheDocument(); expect(screen.getByText(/8 cores available/)).toBeInTheDocument(); expect(screen.getByText("2.0 GiB")).toBeInTheDocument(); expect(screen.getByText("3.0 GiB")).toBeInTheDocument();
    expect(seam.engine).toHaveBeenCalledTimes(1); expect(seam.compose).toHaveBeenCalledTimes(1);
  } else {
    await tick(); expect(seam.engine).not.toHaveBeenCalled(); expect(seam.compose).not.toHaveBeenCalled();
  }
  unsupportedAbsent();
  for (const [name, page] of [[/containers/i, "containers"], [/images/i, "images"], [/volumes/i, "volumes"], [/networks/i, "networks"], [/compose/i, "compose"]] as const) {
    await fireEvent.click(screen.getByRole("button", { name })); expect(navigate).toHaveBeenLastCalledWith(page);
  }
});

it.each(["macos", "linux"] as const)("preserves %s infrastructure reads, VM fallback and navigation", async (os) => {
  host(os); seam.engine.mockRejectedValueOnce(new Error("fixture engine unavailable"));
  const navigate = vi.fn(); render(Dashboard, { systemInfo, loading: false, onNavigate: navigate });
  await waitFor(() => expect(seam.pods).toHaveBeenCalledTimes(1));
  expect(seam.kind).toHaveBeenCalledTimes(1); expect(seam.lima).toHaveBeenCalledTimes(1);
  expect(screen.getByText("VM allocation (engine unreachable)")).toBeInTheDocument(); expect(screen.getByText("stale-vm")).toBeInTheDocument();
  for (const [name, page] of [[/running vms/i, "instances"], [/kubernetes/i, "kubernetes"], [/kind clusters/i, "instances"], [/linux vms/i, "linux-vms"]] as const) {
    await fireEvent.click(screen.getByRole("button", { name })); expect(navigate).toHaveBeenLastCalledWith(page);
  }
});

it("shows unavailable engine resources rather than inventing Windows VM allocations", async () => {
  host("windows"); seam.engine.mockRejectedValueOnce(new Error("fixture failure"));
  dashboardState.colimaInstances = [];
  render(Dashboard, { systemInfo, loading: false, onNavigate: vi.fn() });
  await waitFor(() => expect(seam.engine).toHaveBeenCalledTimes(1)); await tick();
  expect(screen.getByRole("status")).toHaveTextContent("Could not load Docker resources"); unsupportedAbsent();
});

// TestSpec=TC-WINUI-002; TestSpec=TC-WINUI-008 — partial count outcome guards.
// BR06 / TC002/008: a ready engine does not prove its independent Compose read
// succeeded. Neither the initial zero nor an old count may look healthy.
it.each([0, 7])("distinguishes a current Compose failure from cached count %s", async (cached) => {
  host("windows"); dashboardState.composeProjectsCount = cached;
  let reject!: (error: Error) => void;
  seam.compose.mockReturnValueOnce(new Promise<unknown[]>((_, no) => { reject = no; }));
  const navigate = vi.fn(); render(Dashboard, { systemInfo, loading: false, onNavigate: navigate });
  const card = screen.getByRole("button", { name: /compose/i });
  expect(within(card).getByText("—")).toBeInTheDocument();
  expect(within(card).getByText("Loading…")).toHaveAttribute("aria-live", "polite");
  expect(within(card).queryByText(String(cached))).not.toBeInTheDocument();
  await waitFor(() => expect(seam.compose).toHaveBeenCalledTimes(1));
  reject(new Error("private Compose failure"));
  await waitFor(() => expect(card).toHaveTextContent("Could not load Compose projects. Open Compose to retry."));
  expect(within(card).getByText("—")).toBeInTheDocument();
  expect(within(card).queryByText(String(cached))).not.toBeInTheDocument();
  expect(within(card).queryByText("Active projects")).not.toBeInTheDocument();
  expect(screen.queryByText("private Compose failure")).not.toBeInTheDocument();
  await fireEvent.click(card); expect(navigate).toHaveBeenLastCalledWith("compose");
});

it.each([0, 3])("shows a successful current Compose count of %s", async (count) => {
  host("windows"); dashboardState.composeProjectsCount = 7;
  seam.compose.mockResolvedValueOnce(Array.from({ length: count }, () => ({})));
  render(Dashboard, { systemInfo, loading: false, onNavigate: vi.fn() });
  const card = screen.getByRole("button", { name: /compose/i });
  await waitFor(() => expect(within(card).getByText(String(count))).toBeInTheDocument());
  expect(within(card).getByText("Active projects")).toBeInTheDocument();
  expect(within(card).queryByText("—")).not.toBeInTheDocument();
  expect(within(card).queryByText("Loading…")).not.toBeInTheDocument();
});

it("hides the last successful Compose count while a later current read is pending", async () => {
  host("windows"); seam.compose.mockResolvedValueOnce([{}, {}, {}]);
  render(Dashboard, { systemInfo, loading: false, onNavigate: vi.fn() });
  const card = screen.getByRole("button", { name: /compose/i });
  await waitFor(() => expect(within(card).getByText("3")).toBeInTheDocument());
  let resolve!: (rows: unknown[]) => void;
  seam.compose.mockReturnValueOnce(new Promise<unknown[]>((yes) => { resolve = yes; }));
  dashboardState.lastFetch = 0;
  // A capabilities refresh is a real effect trigger; the first-fetch branch
  // does not subscribe to lastFetch before its short-circuit succeeds.
  capabilitiesState.platform = { ...capabilitiesState.platform! };
  await waitFor(() => expect(seam.compose).toHaveBeenCalledTimes(2));
  expect(within(card).getByText("—")).toBeInTheDocument();
  expect(within(card).getByText("Loading…")).toBeInTheDocument();
  expect(within(card).queryByText("3")).not.toBeInTheDocument();
  resolve([{}, {}]);
  await waitFor(() => expect(within(card).getByText("2")).toBeInTheDocument());
});

it.each(["macos", "linux"] as const)("preserves the %s cached Compose count after a transient read failure", async (os) => {
  host(os); dashboardState.composeProjectsCount = 7;
  seam.compose.mockRejectedValueOnce(new Error("legacy transient failure"));
  render(Dashboard, { systemInfo, loading: false, onNavigate: vi.fn() });
  await waitFor(() => expect(seam.compose).toHaveBeenCalledTimes(1)); await tick();
  const card = screen.getByRole("button", { name: /compose/i });
  expect(within(card).getByText("7")).toBeInTheDocument();
  expect(within(card).getByText("Active projects")).toBeInTheDocument();
  expect(within(card).queryByText("—")).not.toBeInTheDocument();
});

it.each(["macos", "linux"] as const)("retains the empty %s VM creation action", async (os) => {
  host(os); dashboardState.colimaInstances = [];
  const navigate = vi.fn(); render(Dashboard, { systemInfo, loading: false, onNavigate: navigate });
  await fireEvent.click(screen.getByRole("button", { name: /create instance/i })); expect(navigate).toHaveBeenCalledWith("instances");
});

// TestSpec=TC-WINUI-035 — partial current-view publication contract.
it.each(["engine", "compose"].flatMap(kind => ["detached", "generation", "unavailable"].flatMap(state => ["resolve", "reject"].map(outcome => ({ kind, state, outcome })))))("fences held Dashboard settlements: %j", async ({ kind, state, outcome }) => {
  host("windows");
  let resolve!: (value: unknown) => void, reject!: (error: Error) => void;
  const held = new Promise<unknown>((yes, no) => { resolve = yes; reject = no; });
  (kind === "engine" ? seam.engine : seam.compose).mockReturnValueOnce(held);
  const old = render(Dashboard, { systemInfo, loading: false, onNavigate: vi.fn() });
  await waitFor(() => expect(seam.engine).toHaveBeenCalledTimes(1));
  await tick();
  const current = { ...engine, engineName: "current-generation" };
  if (state === "detached") {
    // Same-generation navigation: only mount ownership distinguishes this read.
    old.unmount();
    seam.engine.mockResolvedValueOnce(current); seam.compose.mockResolvedValueOnce([{}, {}]);
    render(Dashboard, { systemInfo, loading: false, onNavigate: vi.fn() });
    await waitFor(() => expect(dashboardState.composeProjectsCount).toBe(2));
    await waitFor(() => expect(screen.getByText(/current-generation/)).toBeInTheDocument());
  } else {
    // Connection changes before the shell's keyed-view teardown settles.
    if (state === "generation") { dockerState.originGeneration = 2; engineConnectionState.snapshot!.active.generation = 2; }
    else engineConnectionState.snapshot!.active.ready = false;
    dashboardState.engineResources = current; dashboardState.composeProjectsCount = 2;
  }
  if (outcome === "resolve") resolve(kind === "engine" ? { ...engine, engineName: "obsolete-generation" } : [{}, {}, {}]);
  else reject(new Error("obsolete failure"));
  await tick();
  expect(dashboardState.engineResources).toEqual(current);
  expect(dashboardState.composeProjectsCount).toBe(2);
  expect(screen.queryByText(/obsolete-generation/)).not.toBeInTheDocument();
  expect(seam.engine).toHaveBeenCalledTimes(state === "detached" ? 2 : 1);
});
