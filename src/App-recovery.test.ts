/// <reference types="node" />
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import process from "node:process";
import { setImmediate } from "node:timers";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { tick } from "svelte";
import type { DockerContainer, DockerImage } from "./lib/api";
import type { MetricsHandlers, MetricSample } from "./lib/api/metrics";

const seam = vi.hoisted(() => ({
  containers: vi.fn(), images: vi.fn(), networks: vi.fn(), volumes: vi.fn(),
  run: vi.fn(), pull: vi.fn(), createNetwork: vi.fn(), createVolume: vi.fn(),
  loadArchive: vi.fn(), saveImages: vi.fn(), copyIn: vi.fn(), copyOut: vi.fn(),
  setting: vi.fn(),
  inspectImage: vi.fn(), inspectNetwork: vi.fn(), inspectVolume: vi.fn(), logs: vi.fn(),
  topology: vi.fn(), projects: vi.fn(), services: vi.fn(), projectLogs: vi.fn(), diagnose: vi.fn(),
  feed: vi.fn(), exportFeed: vi.fn(), top: vi.fn(), audit: vi.fn(),
  metrics: null as MetricsHandlers | null,
  publications: false, connection: vi.fn(), handlers: new Map<string, (event: { payload: unknown }) => void>(),
  terminalTarget: vi.fn(),
}));
vi.mock("./lib/api", () => ({
  dockerApi: { listContainers: seam.containers, listImages: seam.images, runContainer: seam.run, pullImage: seam.pull, inspectImage: seam.inspectImage, containerLogs: seam.logs },
  networksApi: { listNetworks: seam.networks, createNetwork: seam.createNetwork, inspectNetwork: seam.inspectNetwork },
  volumesApi: { listVolumes: seam.volumes, createVolume: seam.createVolume, inspectVolume: seam.inspectVolume },
  topologyApi: { getTopology: seam.topology },
  composeApi: { list: seam.projects, ps: seam.services, logs: seam.projectLogs, diagnose: seam.diagnose },
  activityApi: { feed: seam.feed, export: seam.exportFeed },
  sysMethods: { getRuntimeInfo: vi.fn().mockResolvedValue("docker"), checkSystem: vi.fn().mockResolvedValue({}) },
  systemApi: { engineResources: vi.fn().mockResolvedValue(null) }, settingsApi: {},
}));
vi.mock("./lib/api/metrics", () => ({
  subscribeMetrics: (handlers: MetricsHandlers) => { seam.metrics = handlers; return () => { seam.metrics = null; }; },
  metricsApi: { containerTop: seam.top, setInterval: vi.fn() },
}));
vi.mock("./lib/api/security", () => ({ securityApi: { audit: seam.audit, rules: vi.fn().mockResolvedValue(null), alternatives: vi.fn().mockResolvedValue(null) } }));
vi.mock("./components/activity/HealSuggestionBanner.svelte", () => ({ default: () => {} }));
// SVG layout is outside intent retention; the actual Topology filters/detail run.
vi.mock("./components/topology/GraphCanvas.svelte", () => ({ default: () => {} }));
vi.mock("./lib/api/transfer", () => ({ transferApi: {
  loadImages: seam.loadArchive, saveImages: seam.saveImages,
  copyToContainer: seam.copyIn, copyFromContainer: seam.copyOut,
} }));
vi.mock("./lib/settingsStore.svelte", async (original) => ({
  ...await original<typeof import("./lib/settingsStore.svelte")>(),
  loadAllSettings: vi.fn().mockResolvedValue(undefined),
  getAppSetting: (key: string, fallback = "") => ["colimaui_setup_complete", "colimaui_tour_complete"].includes(key) ? "true" : fallback,
  setAppSetting: seam.setting,
}));
vi.mock("./lib/dataPoller", async (original) => {
  const owner = await original<typeof import("./lib/dataPoller")>();
  return {
    startDataPoller: () => seam.publications ? owner.startDataPoller() : vi.fn(),
    refreshManual: () => seam.publications ? owner.refreshManual() : Promise.resolve(),
    refetchAllResources: () => seam.publications ? owner.refetchAllResources() : Promise.resolve(),
  };
});
vi.mock("./lib/env", () => ({ isRunningInTauri: () => true }));
vi.mock("./lib/terminal-transport", async (original) => ({
  ...await original<typeof import("./lib/terminal-transport")>(), containerTerminalTarget: seam.terminalTarget,
}));
vi.mock("./lib/visibleInterval", () => ({ setVisibleInterval: () => () => {} }));
vi.mock("./lib/api/system", () => ({ connectionApi: { get: seam.connection } }));
vi.mock("./store/capabilities.svelte", async (original) => ({
  ...await original<typeof import("./store/capabilities.svelte")>(), loadCapabilities: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => { seam.handlers.set(name, handler); return () => { if (seam.handlers.get(name) === handler) seam.handlers.delete(name); }; }) }));
vi.mock("./lib/transferNotifications", () => ({ startTransferNotifications: () => vi.fn() }));
vi.mock("./lib/announcements", () => ({ startAnnouncements: () => vi.fn() }));
vi.mock("./lib/crashReporter", () => ({ installCrashReporter: vi.fn() }));
vi.mock("./lib/globalToast", () => ({ globalToast: vi.fn() }));
vi.mock("./lib/errorReporter", () => ({ reportError: vi.fn(), withErrorReport: (fn: unknown) => fn }));
vi.mock("./lib/presetStateManager", () => ({
  getCurrentPresetForInstance: () => null, getContainerPresetMap: vi.fn().mockResolvedValue(new Map()), PRESET_LABELS: {}, PRESET_COLORS: {},
}));
// The real shell admission, rune stores, resource forms and TransferDialog run.
// Unrelated views/panels and external transports are outside this DOM seam.
vi.mock("./components/Sidebar.svelte", () => ({ default: () => {} }));
vi.mock("./components/AiChatPanel.svelte", () => ({ default: () => {} }));
vi.mock("./components/notifications/NotificationPanel.svelte", () => ({ default: () => {} }));
vi.mock("./components/ErrorDetailPanel.svelte", () => ({ default: () => {} }));
vi.mock("./components/ToastContainer.svelte", () => ({ default: () => {} }));
vi.mock("./components/ConfirmDialog.svelte", () => ({ default: () => {} }));
vi.mock("./components/SetupWizard.svelte", () => ({ default: () => {} }));
vi.mock("./components/GettingStartedTour.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Dashboard.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Instances.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Models.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Kubernetes.svelte", () => ({ default: () => {} }));
vi.mock("./pages/LinuxVMs.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Settings.svelte", () => ({ default: () => {} }));
vi.mock("./pages/TerminalInstance.svelte", () => ({ default: () => {} }));
vi.mock("./pages/Help.svelte", () => ({ default: () => {} }));

import App from "./App.svelte";
import { dockerState, resourceState, uiState } from "./store.svelte";
import { capabilitiesState } from "./store/capabilities.svelte";
import { engineConnectionState, markEngineUnavailable } from "./lib/engineConnection.svelte";
import { _resetNotificationsForTest, notificationState } from "./store/notifications.svelte";
import { globalToast } from "./lib/globalToast";

// Use the actual cold default, so an eager production certificate cannot be
// hidden by a fixture that repairs it before rendering App.
const coldResourceGeneration = resourceState.loadedGeneration;
const container: DockerContainer = { Id: "a".repeat(64), Names: "fixture", Image: "fixture:v1", State: "running", Status: "Up", Ports: "", CreatedAt: "", Size: "", Command: "" };
const image: DockerImage = { Id: "sha256:" + "b".repeat(64), Repository: "fixture", Tag: "v1", Size: "1MB", CreatedAt: "" };
function ready(generation = 1, engineId = "original-engine") {
  engineConnectionState.snapshot = {
    active: { context: "fixture", endpoint: "npipe:////./pipe/fixture", engine_id: engineId, generation, ready: true, error: null },
    pending: null, contexts: [], discovery_error: null,
  };
  dockerState.originGeneration = generation;
  resourceState.loadedGeneration = generation;
  dockerState.loading = false; dockerState.error = null;
  dockerState.containers = [container]; dockerState.images = [image];
  resourceState.networksLoading = false; resourceState.volumesLoading = false;
}
async function lose() {
  markEngineUnavailable();
  dockerState.originGeneration = null;
  dockerState.containers = []; dockerState.images = [];
  await tick();
  expect(screen.getByRole("heading", { name: /engine unavailable/i })).toBeInTheDocument();
}
const type = (placeholder: string | RegExp, value: string) => fireEvent.input(screen.getByPlaceholderText(placeholder), { target: { value } });
function expectInput(placeholder: string | RegExp, value: string) {
  const input = screen.queryByPlaceholderText(placeholder);
  expect(input, "the recovered form must show its typed field").toBeInTheDocument();
  expect(input).toHaveValue(value);
}
const runSubmit = () => within(screen.getByRole("heading", { name: "Run Container" }).closest<HTMLElement>(".modal")!).getByRole("button", { name: "Run Container" });
function noEffects() {
  for (const call of [seam.run, seam.pull, seam.createNetwork, seam.createVolume, seam.loadArchive, seam.saveImages, seam.copyIn, seam.copyOut]) expect(call).not.toHaveBeenCalled();
}
beforeEach(() => {
  vi.clearAllMocks(); localStorage.clear(); _resetNotificationsForTest();
  for (const value of Object.values(seam)) if (vi.isMockFunction(value)) value.mockReset();
  seam.containers.mockResolvedValue([container]); seam.images.mockResolvedValue([image]);
  seam.networks.mockResolvedValue([]); seam.volumes.mockResolvedValue([]);
  seam.pull.mockReset().mockResolvedValue(undefined);
  seam.inspectImage.mockResolvedValue("fresh image inspection"); seam.inspectNetwork.mockResolvedValue("fresh network inspection"); seam.inspectVolume.mockResolvedValue("fresh volume inspection");
  seam.logs.mockResolvedValue("fresh container logs"); seam.top.mockResolvedValue("fresh processes");
  seam.topology.mockResolvedValue({ nodes: [], edges: [], warnings: [] }); seam.projects.mockResolvedValue([]);
  seam.services.mockResolvedValue("fresh services"); seam.projectLogs.mockResolvedValue("fresh project logs");
  seam.feed.mockResolvedValue({ items: [], partial: [] }); seam.metrics = null;
  seam.publications = false; seam.handlers.clear();
  seam.connection.mockImplementation(async () => ({ ...engineConnectionState.snapshot, active: { ...engineConnectionState.snapshot!.active }, contexts: [] }));
  capabilitiesState.platform = { os: "windows", arch: "x86_64", wsl: false, wsl_available: false, package_managers: [] };
  capabilitiesState.items = [{ id: "docker", name: "Docker", state: "running" }];
  capabilitiesState.failed = false; capabilitiesState.loading = false;
  engineConnectionState.loading = false;
  uiState.currentPage = "containers"; uiState.globalError = null; uiState.focusResource = null; uiState.pendingContainerLogs = null;
  resourceState.networks = []; resourceState.volumes = [];
  ready();
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

// BR06 / TC018: preserve operator work without retaining stale resources or
// re-running an operation. These are DOM boundaries, not native engine proof.
it("restores exact unsent Run fields after original-engine loss and recovery", async () => {
  render(App);
  await fireEvent.click(screen.getByRole("button", { name: /^Run$/i }));
  const fields = [
    ["Image (e.g. nginx:latest) *", "fixture:v1"],
    ["Container Name (optional)", "工具-name"],
    ["Ports (host:container, one per line)", "8080:80\n9000:90"],
    ["Environment Variables (KEY=VALUE, one per line)", "TOKEN=private-draft"],
    ["Volumes (host:container, one per line)", "C:\\private\\資料:/data"],
  ];
  for (const [placeholder, value] of fields) await type(placeholder, value);
  await fireEvent.click(screen.getByRole("checkbox", { name: "Detached mode" }));
  await fireEvent.click(screen.getByRole("checkbox", { name: "Remove on exit" }));
  await lose();
  expect(screen.queryByPlaceholderText(fields[0][0])).not.toBeInTheDocument();
  noEffects(); ready(2); await tick();
  for (const [placeholder, value] of fields) expectInput(placeholder, value);
  expect(screen.getByRole("checkbox", { name: "Detached mode" })).not.toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Remove on exit" })).toBeChecked();
  noEffects();
  expect(JSON.stringify(localStorage)).not.toContain("private-draft");
  expect(JSON.stringify(notificationState.entries)).not.toContain("private-draft");
  expect(seam.setting).not.toHaveBeenCalled();
  await fireEvent.click(runSubmit());
  await waitFor(() => expect(seam.run).toHaveBeenCalledWith("fixture:v1", "工具-name", ["8080:80", "9000:90"], ["TOKEN=private-draft"], ["C:\\private\\資料:/data"], false, true));
});

it("restores an open transfer and its private typed path without starting it", async () => {
  uiState.currentPage = "images"; render(App);
  await fireEvent.click(screen.getByRole("button", { name: /import tar/i }));
  const path = "C:\\Users\\private-account\\資料\\images.tar";
  await type(/images\.tar/, path);
  await lose(); noEffects(); ready(2); await tick();
  expectInput(/images\.tar/, path);
  expect(screen.getByRole("button", { name: /start in background/i })).toBeEnabled();
  noEffects(); expect(JSON.stringify(localStorage)).not.toContain("private-account");
  expect(JSON.stringify(notificationState.entries)).not.toContain("private-account");
  await fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
  await fireEvent.click(screen.getByRole("button", { name: /import tar/i }));
  expectInput(/images\.tar/, "");
});

it.each(["export", "copy-in", "copy-out"])("retains the %s transfer fields and overwrite choice across recovery", async (mode) => {
  uiState.currentPage = mode === "export" ? "images" : "containers";
  render(App);
  if (mode === "export") {
    await fireEvent.click(screen.getAllByRole("checkbox")[0]);
    await fireEvent.click(screen.getByRole("button", { name: /export tar/i }));
  } else {
    await fireEvent.click(screen.getByRole("button", { name: /more actions/i }));
    await fireEvent.click(screen.getByRole("button", { name: mode === "copy-in" ? /^Copy file into container$/i : /^Copy file from container$/i }));
  }
  const fields = mode === "copy-in"
    ? [["C:\\Users\\name\\data.json", "C:\\private\\資料.json"], ["/app/data.json", "/app/資料.json"]]
    : [["C:\\Users\\name\\Downloads", "\\\\server\\private\\資料"], ["images.tar", "資料.tar"], ...(mode === "copy-out" ? [["/app/data.json", "/app/資料.json"]] : [])];
  for (const [placeholder, value] of fields) await type(placeholder, value);
  if (mode !== "copy-in") await fireEvent.click(within(screen.getByRole("dialog")).getByRole("checkbox"));
  await lose(); noEffects(); ready(2); await tick();
  for (const [placeholder, value] of fields) expectInput(placeholder, value);
  if (mode !== "copy-in") expect(within(screen.getByRole("dialog")).getByRole("checkbox")).toBeChecked();
  noEffects();
});

it("never applies a removed container's transfer draft to a different target", async () => {
  render(App);
  await fireEvent.click(screen.getByRole("button", { name: /more actions/i }));
  await fireEvent.click(screen.getByRole("button", { name: /^Copy file into container$/i }));
  await type("C:\\Users\\name\\data.json", "C:\\private\\old-target.json");
  await type("/app/data.json", "/old-target.json");
  await lose();
  const replacement = { ...container, Id: "c".repeat(64), Names: "replacement" };
  seam.containers.mockResolvedValue([replacement]); ready(2); dockerState.containers = [replacement]; await tick();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  await fireEvent.click(screen.getByRole("button", { name: /more actions/i }));
  await fireEvent.click(screen.getByRole("button", { name: /^Copy file into container$/i }));
  expect(screen.getByPlaceholderText("C:\\Users\\name\\data.json")).toHaveValue("");
  expect(screen.getByPlaceholderText("/app/data.json")).toHaveValue(""); noEffects();
});

// TestSpec=TC-WINUI-012; TestSpec=TC-WINUI-018; TestSpec=TC-WINUI-035 — partial export recovery.
// TC012/018/035: recovery cannot turn an explicit export of content A into
// content B under the same mutable tag, including a later fresh dialog mount.
it.each(["removed", "retagged", "renamed-reference", "one-of-many-replaced"])("requires fresh export intent when image content is %s", async (variant) => {
  uiState.currentPage = "images";
  const second = { ...image, Id: "sha256:" + "d".repeat(64), Repository: "second" };
  if (variant === "one-of-many-replaced") dockerState.images = [image, second];
  render(App);
  await fireEvent.click(screen.getAllByRole("checkbox")[0]);
  await fireEvent.click(screen.getByRole("button", { name: /export tar/i }));
  await type("C:\\Users\\name\\Downloads", "C:\\private\\original-content");
  await type("images.tar", "original-content.tar");
  await fireEvent.click(within(screen.getByRole("dialog")).getByRole("checkbox"));
  await lose();
  const replacement = { ...image, Id: "sha256:" + "e".repeat(64) };
  const fresh = variant === "removed" ? [] : variant === "one-of-many-replaced" ? [image, { ...second, Id: replacement.Id }] : variant === "renamed-reference" ? [{ ...image, Tag: "v2" }] : [replacement];
  seam.images.mockResolvedValue(fresh); ready(2); dockerState.images = fresh; await tick();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  noEffects();
  // A new explicit open must also refuse the child's old private path/overwrite
  // cache, even if its tag strings still equal the previous export's props.
  dockerState.images = variant === "removed" ? [replacement] : fresh;
  await tick();
  const selectAll = screen.getAllByRole("checkbox")[0] as HTMLInputElement;
  if (!selectAll.checked) await fireEvent.click(selectAll);
  await fireEvent.click(screen.getByRole("button", { name: /export tar/i }));
  expectInput("C:\\Users\\name\\Downloads", "");
  expectInput("images.tar", "images.tar");
  expect(within(screen.getByRole("dialog")).getByRole("checkbox")).not.toBeChecked();
  noEffects();
  await type("C:\\Users\\name\\Downloads", "C:\\private\\new-explicit-export");
  seam.saveImages.mockResolvedValue({ jobId: "new-export", totalEstimate: null });
  await fireEvent.click(screen.getByRole("button", { name: /start in background/i }));
  await waitFor(() => expect(seam.saveImages).toHaveBeenCalledWith(dockerState.images.map(row => `${row.Repository}:${row.Tag}`), "C:\\private\\new-explicit-export", "images.tar", false));
});

it.each(["resource-error", "generation-replacement"])("preserves drafts across %s without keeping the old page active", async (fault) => {
  render(App); await fireEvent.click(screen.getByRole("button", { name: /^Run$/i }));
  await type("Container Name (optional)", "retained-draft");
  if (fault === "resource-error") {
    dockerState.error = "Fixture resource query unavailable"; await tick();
    expect(screen.queryByPlaceholderText("Container Name (optional)")).not.toBeInTheDocument();
    noEffects(); ready(1);
  } else ready(2);
  await tick();
  expectInput("Container Name (optional)", "retained-draft"); noEffects();
  // An old keyed branch's late cleanup must not unregister the new page.
  await lose(); ready(3); await tick();
  expectInput("Container Name (optional)", "retained-draft"); noEffects();
});

it.each([
  { page: "networks", open: /create network/i, placeholder: "my-network", name: "draft-network" },
  { page: "volumes", open: /create volume/i, placeholder: "my-volume", name: "draft-volume" },
])("keeps the unsent $page creation draft across recovery", async ({ page, open, placeholder, name }) => {
  uiState.currentPage = page; render(App);
  await fireEvent.click(screen.getByRole("button", { name: open }));
  await type(placeholder, name);
  if (page === "networks") await type("172.28.0.0/16", "172.29.0.0/16");
  await lose(); noEffects(); ready(2); await tick();
  expectInput(placeholder, name);
  if (page === "networks") expect(screen.getByPlaceholderText("172.28.0.0/16")).toHaveValue("172.29.0.0/16");
  noEffects();
});

it.each(["engine_id", "context", "endpoint"] as const)("discards drafts when the active %s changes", async (field) => {
  render(App); await fireEvent.click(screen.getByRole("button", { name: /^Run$/i }));
  await type("Container Name (optional)", "original-only");
  await lose(); ready(2); engineConnectionState.snapshot!.active[field] = "different-origin"; await tick();
  expect(screen.queryByPlaceholderText("Container Name (optional)")).not.toBeInTheDocument();
  await fireEvent.click(screen.getByRole("button", { name: /^Run$/i }));
  expect(screen.getByPlaceholderText("Container Name (optional)")).toHaveValue(""); noEffects();
});

it("discards recovery drafts after voluntary navigation or App destruction", async () => {
  const app = render(App); await fireEvent.click(screen.getByRole("button", { name: /^Run$/i }));
  await type("Container Name (optional)", "not-persisted");
  await lose(); uiState.currentPage = "images"; await tick(); ready(2); await tick();
  uiState.currentPage = "containers"; await tick();
  await fireEvent.click(screen.getByRole("button", { name: /^Run$/i }));
  expect(screen.getByPlaceholderText("Container Name (optional)")).toHaveValue("");
  await type("Container Name (optional)", "not-persisted");
  await lose(); app.unmount(); ready(3); render(App);
  expect(screen.queryByPlaceholderText("Container Name (optional)")).not.toBeInTheDocument(); noEffects();
});

async function openPull(name: string) {
  uiState.currentPage = "images"; render(App);
  await fireEvent.click(screen.getByRole("button", { name: "Pull Image" }));
  await fireEvent.input(screen.getByLabelText("Image name"), { target: { value: name } });
}
const pullButton = () => screen.getByRole("button", { name: "Pull" });

// Node/jsdom reports escaped promises on process; a browser delivers them to
// window. Keep this adapter local: Vitest defers to extra process listeners,
// so EVERY captured rejection must be asserted, including unrelated failures.
async function withBrowserRejections(action: () => Promise<void>, expected: unknown[] = []) {
  const escaped: unknown[] = [];
  const deliver = (reason: unknown, promise: Promise<unknown>) => {
    escaped.push(reason);
    window.dispatchEvent(new window.PromiseRejectionEvent("unhandledrejection", { reason, promise, cancelable: true }));
  };
  process.on("unhandledRejection", deliver);
  try {
    await action();
    // Cross the host notification task after the operation's microtasks, then
    // let the real App boundary render. This is not a timed assertion retry.
    await new Promise<void>((done) => { setImmediate(done); });
    await tick();
    expect(escaped).toEqual(expected);
  } finally {
    process.off("unhandledRejection", deliver);
  }
}

it("delivers an escaped promise to the real App error boundary and releases only its listener", async () => {
  const listeners = process.listeners("unhandledRejection");
  const failure = new Error("owned browser rejection control");
  await openPull("fixture:draft");
  await withBrowserRejections(async () => { void Promise.reject(failure); }, [failure]);
  expect(screen.getByRole("heading", { name: failure.message })).toBeInTheDocument();
  expect(screen.queryByLabelText("Image name")).not.toBeInTheDocument();
  expect(process.listeners("unhandledRejection")).toEqual(listeners);
});

it("fails the host adapter for an unrelated rejection and releases its listener after assertion failure", async () => {
  const listeners = process.listeners("unhandledRejection");
  const failure = new Error("unexpected unrelated rejection control");
  await openPull("fixture:draft");
  await expect(withBrowserRejections(async () => { void Promise.reject(failure); })).rejects.toThrow();
  expect(screen.getByRole("heading", { name: failure.message })).toBeInTheDocument();
  expect(process.listeners("unhandledRejection")).toEqual(listeners);
});

// TC006/018: rejected resource actions keep their input; only the completed
// matching submission may clear it. Automation never owns this user draft.
it.each(["windows", "macos", "linux"] as const)("keeps a rejected pull usable on %s and retries only after another explicit submission", async (os) => {
  capabilitiesState.platform!.os = os;
  if (os !== "windows") capabilitiesState.items.push({ id: "colima", name: "Colima", state: "running" });
  seam.pull.mockRejectedValueOnce(new Error("fixture registry unavailable"));
  await openPull("fixture:missing");
  await withBrowserRejections(async () => {
    await fireEvent.click(pullButton());
    await waitFor(() => expect(globalToast).toHaveBeenCalledWith("error", expect.stringContaining("fixture registry unavailable")));
  });
  await waitFor(() => expect(globalToast).toHaveBeenCalledWith("error", expect.stringContaining("fixture registry unavailable")));
  expect(screen.queryByLabelText("Image name")).toBeInTheDocument();
  expect(screen.getByLabelText("Image name")).toHaveValue("fixture:missing");
  expect(pullButton()).toBeEnabled(); expect(seam.pull).toHaveBeenCalledTimes(1);
  await fireEvent.input(screen.getByLabelText("Image name"), { target: { value: "fixture:retry" } });
  await fireEvent.click(pullButton());
  await waitFor(() => expect(screen.queryByLabelText("Image name")).not.toBeInTheDocument());
  expect(seam.pull.mock.calls.map(([name]) => name)).toEqual(["fixture:missing", "fixture:retry"]);
  await fireEvent.click(screen.getByRole("button", { name: "Pull Image" }));
  expect(screen.getByLabelText("Image name")).toHaveValue("");
});

it.each(["fixture:newer-draft", "fixture:original"])("does not clear an edited draft on an older pull completion: %s", async (nextDraft) => {
  let finish!: () => void;
  seam.pull.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; }));
  await openPull("fixture:original"); await fireEvent.click(pullButton());
  expect(pullButton()).toBeDisabled();
  await fireEvent.keyDown(screen.getByLabelText("Image name"), { key: "Enter" });
  expect(seam.pull).toHaveBeenCalledTimes(1);
  await fireEvent.input(screen.getByLabelText("Image name"), { target: { value: "fixture:intermediate" } });
  await fireEvent.input(screen.getByLabelText("Image name"), { target: { value: nextDraft } });
  finish();
  await waitFor(() => expect(screen.queryByLabelText("Image name")).toBeInTheDocument());
  await waitFor(() => expect(pullButton()).toBeEnabled());
  expect(screen.getByLabelText("Image name")).toHaveValue(nextDraft);
  expect(seam.pull).toHaveBeenCalledTimes(1);
});

it.each([false, true])("preserves user pull input and truthful automation outcome (failure=%s)", async (failed) => {
  if (failed) seam.pull.mockRejectedValueOnce(new Error("fixture automation rejection"));
  await openPull("fixture:user-draft");
  const resolve = vi.fn();
  window.dispatchEvent(new CustomEvent("pull-image", { detail: { payload: "fixture:automation", resolve } }));
  await waitFor(() => expect(resolve).toHaveBeenCalledTimes(1));
  expect(resolve).toHaveBeenCalledWith(expect.stringContaining(failed ? "Failed" : "Successfully"));
  expect(screen.queryByLabelText("Image name")).toBeInTheDocument();
  expect(screen.getByLabelText("Image name")).toHaveValue("fixture:user-draft");
  expect(seam.pull).toHaveBeenCalledWith("fixture:automation");
});

// BR06 / TC018: selection is neutral intent, not an old resource/result.
const network = { Id: "network-id", Name: "fixture-network", Driver: "bridge", Scope: "local", Ipv6: "false", Internal: "false", Labels: "" };
const volume = { Name: "fixture-volume", Driver: "local", Mountpoint: "/fixture", Scope: "local", Labels: "" };
function resourceFixtures() {
  resourceState.networks = [network]; resourceState.volumes = [volume];
  seam.networks.mockResolvedValue([network]); seam.volumes.mockResolvedValue([volume]);
}
const lastCheckbox = () => screen.getAllByRole("checkbox").slice(-1)[0];
async function openInspection(page: string) {
  if (page === "images") await fireEvent.click(screen.getByRole("button", { name: "Inspect" }));
  else await fireEvent.click(screen.getByText(page === "networks" ? network.Name : volume.Name));
}
it.each(["containers", "images", "networks", "volumes"])("restores $page checked rows and search, then clears rows on a new search", async (page) => {
  resourceFixtures(); uiState.currentPage = page; render(App);
  await type(/Search (containers|images|networks|volumes)/i, "fixture");
  await fireEvent.click(lastCheckbox()); expect(lastCheckbox()).toBeChecked();
  await lose(); resourceState.networks = []; resourceState.volumes = [];
  ready(2); resourceFixtures(); await tick();
  expectInput(/Search (containers|images|networks|volumes)/i, "fixture");
  expect(lastCheckbox()).toBeChecked(); noEffects();
  await type(/Search (containers|images|networks|volumes)/i, "");
  expect(lastCheckbox()).not.toBeChecked();
});

it.each(["images", "networks", "volumes"])("reopens $page inspection using fresh output rather than cached output", async (page) => {
  resourceFixtures(); uiState.currentPage = page;
  const inspect = page === "images" ? seam.inspectImage : page === "networks" ? seam.inspectNetwork : seam.inspectVolume;
  inspect.mockResolvedValueOnce("old-private-inspection").mockResolvedValue("fresh-inspection");
  render(App); await openInspection(page);
  await screen.findByText("old-private-inspection");
  await lose(); ready(2); resourceFixtures(); await tick();
  await screen.findByText("fresh-inspection");
  expect(screen.queryByText("old-private-inspection")).not.toBeInTheDocument();
  expect(inspect).toHaveBeenCalledTimes(2); noEffects();
});

it("restores the chosen container and Logs tab using a new read, never old output", async () => {
  seam.logs.mockResolvedValueOnce("old-private-logs").mockResolvedValue("fresh-logs");
  render(App); await fireEvent.click(screen.getByText("fixture"));
  await fireEvent.click(screen.getByRole("button", { name: "logs" }));
  await screen.findByText("old-private-logs");
  await lose(); ready(2); await tick();
  await screen.findByText("fresh-logs");
  expect(screen.queryByText("old-private-logs")).not.toBeInTheDocument(); noEffects();
});

it.each(["containers", "images", "networks", "volumes"])("never restores checked identities absent from fresh $page", async (page) => {
  resourceFixtures(); uiState.currentPage = page; render(App);
  await fireEvent.click(lastCheckbox()); await lose(); ready(2);
  if (page === "containers") { seam.containers.mockResolvedValue([]); dockerState.containers = []; }
  if (page === "images") { seam.images.mockResolvedValue([]); dockerState.images = []; }
  if (page === "networks") { seam.networks.mockResolvedValue([]); resourceState.networks = []; }
  if (page === "volumes") { seam.volumes.mockResolvedValue([]); resourceState.volumes = []; }
  await tick();
  if (page === "images") expect(screen.getByRole("button", { name: /^Export TAR$/ })).toBeDisabled();
  else expect(screen.queryByRole("button", { name: /remove selected|stop selected/i })).not.toBeInTheDocument();
  for (const checkbox of screen.queryAllByRole("checkbox")) expect(checkbox).not.toBeChecked();
  noEffects();
});

it.each(["containers", "images", "networks", "volumes"])("grants no stale selection authority when fresh $page contains only unrelated identities", async (page) => {
  resourceFixtures(); uiState.currentPage = page; render(App);
  await fireEvent.click(lastCheckbox()); expect(lastCheckbox()).toBeChecked();
  await lose(); ready(2);
  // An external actor replaced the selected resource while the engine was
  // unavailable. Nonempty membership must reject the old ID, not just empty lists.
  if (page === "containers") {
    const fresh = [{ ...container, Id: "c".repeat(64), Names: "unrelated-container" }];
    seam.containers.mockResolvedValue(fresh); dockerState.containers = fresh;
  } else if (page === "images") {
    const fresh = [{ ...image, Id: "sha256:" + "c".repeat(64), Repository: "unrelated-image" }];
    seam.images.mockResolvedValue(fresh); dockerState.images = fresh;
  } else if (page === "networks") {
    const fresh = [{ ...network, Id: "unrelated-network-id", Name: "unrelated-network" }];
    seam.networks.mockResolvedValue(fresh); resourceState.networks = fresh;
  } else {
    const fresh = [{ ...volume, Name: "unrelated-volume" }];
    seam.volumes.mockResolvedValue(fresh); resourceState.volumes = fresh;
  }
  await tick();
  expect(screen.getByText(`unrelated-${page === "images" ? "image" : page.slice(0, -1)}`)).toBeInTheDocument();
  for (const checkbox of screen.getAllByRole("checkbox")) expect(checkbox).not.toBeChecked();
  expect(screen.queryByText(/^\d+ selected$/i)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /(?:start|stop|restart|remove).*selected/i })).not.toBeInTheDocument();
  if (page === "images") expect(screen.getByRole("button", { name: /^Export TAR$/ })).toBeDisabled();
  noEffects();
});

it.each([false, true])("retains Topology filters and only a fresh node selection (removed=%s)", async (removed) => {
  uiState.currentPage = "topology"; uiState.focusResource = { page: "topology", id: "container:fixture" };
  const node = { id: "container:fixture", kind: "container", label: "fixture-node", status: "running", meta: {} };
  seam.topology.mockResolvedValue({ nodes: [node], edges: [], warnings: [] }); render(App);
  await screen.findByRole("button", { name: /Open in Containers/i });
  await type("Search by name", "fixture");
  await fireEvent.click(screen.getByRole("button", { name: /^Networks/ }));
  await lose();
  seam.topology.mockResolvedValue({ nodes: removed ? [] : [{ ...node, label: "fresh-node" }], edges: [], warnings: [] });
  ready(2); await tick();
  expectInput("Search by name", "fixture");
  expect(screen.getByRole("button", { name: /^Networks/ })).toHaveAttribute("aria-pressed", "false");
  await waitFor(() => expect(seam.topology).toHaveBeenCalledTimes(2));
  if (removed) expect(screen.queryByRole("button", { name: /Open in Containers/i })).not.toBeInTheDocument();
  else await screen.findByText("fresh-node");
  noEffects();
});

const sample: MetricSample = { ts: 1, instance: "fixture", containerId: container.Id, name: "fixture-live", cpuPct: 1, memBytes: 2, memLimitBytes: 4, memPct: 50, netRxBytes: 0, netTxBytes: 0, blockReadBytes: 0, blockWriteBytes: 0, pids: 1 };
it("restores Activity sort/search/selected container only after a fresh sample", async () => {
  uiState.currentPage = "activity"; render(App);
  seam.metrics!.onBatch!({ samples: [sample] }); await tick();
  await type("Filter by name", "fixture");
  await fireEvent.click(screen.getByRole("button", { name: "Memory" }));
  await fireEvent.click(screen.getByText("fixture-live")); await screen.findByText("fresh processes");
  await lose(); ready(2); await tick();
  expectInput("Filter by name", "fixture");
  expect(screen.getByRole("button", { name: /Memory ▼/ })).toBeInTheDocument();
  expect(screen.queryByText("fresh processes")).not.toBeInTheDocument();
  expect(seam.top).toHaveBeenCalledTimes(1);
  seam.metrics!.onBatch!({ samples: [{ ...sample, ts: 2, name: "fixture-current" }] }); await tick();
  await screen.findByText("fresh processes"); expect(seam.top).toHaveBeenCalledTimes(2); noEffects();
});
it("restores Activity Actions tab and its real nested kind filter without replaying export", async () => {
  seam.feed.mockResolvedValue({ items: [], partial: [] });
  uiState.currentPage = "activity"; render(App);
  await fireEvent.click(screen.getByRole("tab", { name: "Actions" }));
  await fireEvent.click(screen.getByRole("button", { name: "Destructive" }));
  await lose(); ready(2); await tick();
  expect(screen.getByRole("button", { name: "Destructive" })).toHaveClass("active");
  await waitFor(() => expect(seam.feed).toHaveBeenLastCalledWith({ kind: "destructive", limit: 200 }));
  expect(seam.exportFeed).not.toHaveBeenCalled(); noEffects();
});
it.each([false, true])("restores Compose project/tab only from fresh projects (removed=%s)", async (removed) => {
  capabilitiesState.items.push({ id: "docker-compose", name: "Compose", state: "running" });
  const project = { Name: "fixture-project", Status: "running(1)", ConfigFiles: "C:\\資料\\compose.yaml" };
  seam.projects.mockResolvedValue([project]); uiState.currentPage = "compose"; render(App);
  await fireEvent.click(await screen.findByText("fixture-project"));
  await fireEvent.click(screen.getByRole("button", { name: /^Diagnose$/i }));
  await lose(); seam.projects.mockResolvedValue(removed ? [] : [project]); ready(2); await tick();
  await waitFor(() => expect(seam.projects).toHaveBeenCalledTimes(2));
  if (removed) expect(screen.queryByRole("heading", { name: "fixture-project" })).not.toBeInTheDocument();
  else { await screen.findByRole("heading", { name: "fixture-project" }); expect(screen.getByRole("button", { name: "Analyze" })).toBeInTheDocument(); }
  expect(seam.diagnose).not.toHaveBeenCalled(); noEffects();
});

function auditFixture() {
  const component = { earned: 1, max: 1, failedRules: [] };
  return { scan: { imageRef: "fixture:v1", imageDigest: image.Id, findings: [{ id: "fixture-finding", package: "fixture", installedVersion: "1", severity: "low" }], scanner: "trivy", scannerVersion: "fixture", scannedAt: 1 },
    evaluation: { results: [{ ruleId: "fixture-passed", passed: true, severity: "low", component: "hardening", weight: 1 }], skipped: [] },
    score: { vulnerabilities: component, hardening: component, provenance: component, freshness: component, total: 100, inputs: { packVersion: "fixture", engineVersion: "fixture", scanner: "trivy", scannerVersion: "fixture", level: "l2" } } };
}
it.each([false, true])("restores Security neutral choices but requires a fresh explicit scan (replaced=%s)", async (replaced) => {
  uiState.currentPage = "security"; capabilitiesState.items.push({ id: "trivy", name: "Trivy", state: "running" });
  seam.audit.mockResolvedValue(auditFixture()); render(App);
  await fireEvent.change(screen.getByLabelText("Strictness"), { target: { value: "l2" } });
  await fireEvent.click(screen.getByRole("button", { name: "Images" }));
  await fireEvent.click(await screen.findByRole("button", { name: /^fixture:v1(?:\s|$)/ }));
  await fireEvent.click(screen.getByRole("button", { name: "Scan" }));
  await screen.findByText("fixture-finding");
  await fireEvent.change(screen.getByLabelText("Show down to"), { target: { value: "high" } });
  await fireEvent.click(screen.getByRole("button", { name: "Show 1 passing rules" }));
  await lose();
  const freshImage = replaced ? { ...image, Id: "sha256:" + "c".repeat(64) } : image;
  seam.images.mockResolvedValue([freshImage]); ready(2); dockerState.images = [freshImage]; await tick();
  await waitFor(() => expect(seam.images).toHaveBeenCalledTimes(2));
  expect(screen.getByLabelText("Strictness")).toHaveValue("l2");
  expect(seam.audit).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("fixture-finding")).not.toBeInTheDocument();
  expect(screen.queryByText("fixture-passed")).not.toBeInTheDocument();
  if (replaced) {
    expect(screen.queryByRole("button", { name: "Scan" })).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: /^fixture:v1(?:\s|$)/ }));
  }
  await fireEvent.click(screen.getByRole("button", { name: "Scan" }));
  await waitFor(() => expect(seam.audit).toHaveBeenCalledTimes(2));
  await screen.findByLabelText("Show down to");
  expect(screen.getByLabelText("Show down to")).toHaveValue(replaced ? "low" : "high");
  if (replaced) expect(screen.queryByText("fixture-passed")).not.toBeInTheDocument();
  else expect(screen.getByText("fixture-passed")).toBeInTheDocument();
});

// BR02/06 / TC007,035: use actual App + real page + real publication owner.
// Only IPC/event/engine responses are substituted. An isolated poller test
// would not catch a page that bypasses that owner.
it.each(["containers", "images", "networks", "volumes", "terminal"].flatMap(page => [false, true].map(rejected => ({ page, rejected }))))("ignores pre-loss $page reads after current recovery (rejected=$rejected)", async ({ page, rejected }) => {
  resourceFixtures(); seam.publications = true; uiState.currentPage = "help"; render(App);
  await waitFor(() => expect(seam.handlers.has("docker-reconnected")).toBe(true));
  await waitFor(() => expect(seam.containers).toHaveBeenCalledTimes(1));
  const read = page === "images" ? seam.images : page === "networks" ? seam.networks : page === "volumes" ? seam.volumes : seam.containers;
  let resolve!: (rows: unknown[]) => void; let reject!: (error: Error) => void;
  read.mockReturnValueOnce(new Promise<unknown[]>((yes, no) => { resolve = yes; reject = no; }));
  const calls = read.mock.calls.length; uiState.currentPage = page; await tick();
  await waitFor(() => expect(read).toHaveBeenCalledTimes(calls + 1));
  seam.handlers.get("docker-connection-lost")!({ payload: {} }); await tick();
  const freshContainer = { ...container, Id: "d".repeat(64), Names: "recovered-container" };
  const freshImage = { ...image, Id: "sha256:" + "e".repeat(64), Repository: "recovered-image" };
  const freshNetwork = { ...network, Id: "recovered-network", Name: "recovered-network" };
  const freshVolume = { ...volume, Name: "recovered-volume" };
  seam.containers.mockResolvedValue([freshContainer]); seam.images.mockResolvedValue([freshImage]);
  seam.networks.mockResolvedValue([freshNetwork]); seam.volumes.mockResolvedValue([freshVolume]);
  seam.connection.mockResolvedValue({ ...engineConnectionState.snapshot!, active: { ...engineConnectionState.snapshot!.active, ready: true, generation: 2 } });
  seam.handlers.get("docker-reconnected")!({ payload: {} });
  await waitFor(() => expect(dockerState.originGeneration).toBe(2));
  await waitFor(() => expect(dockerState.containers).toEqual([freshContainer]));
  if (rejected) reject(new Error("pre-loss read refused"));
  else resolve(page === "images" ? [image] : page === "networks" ? [network] : page === "volumes" ? [volume] : [container]);
  await tick(); await tick();
  expect(dockerState.containers).toEqual([freshContainer]); expect(dockerState.images).toEqual([freshImage]);
  expect(resourceState.networks).toEqual([freshNetwork]); expect(resourceState.volumes).toEqual([freshVolume]);
  expect(dockerState.error).toBeNull(); expect(dockerState.originGeneration).toBe(2);
  noEffects();
});
it("attributes a current page read failure and distinguishes a subsequent healthy empty engine", async () => {
  seam.publications = true; uiState.currentPage = "help"; render(App);
  await waitFor(() => expect(seam.handlers.has("docker-reconnected")).toBe(true));
  await waitFor(() => expect(seam.networks).toHaveBeenCalledTimes(1));
  seam.networks.mockRejectedValueOnce(new Error("current network failure"));
  uiState.currentPage = "networks"; await tick();
  await waitFor(() => expect(dockerState.error).toBeTruthy());
  expect(screen.queryByText("No networks found")).not.toBeInTheDocument();
  seam.containers.mockResolvedValue([]); seam.images.mockResolvedValue([]); seam.networks.mockResolvedValue([]); seam.volumes.mockResolvedValue([]);
  await (await import("./lib/dataPoller")).refetchAllResources(); await tick();
  expect(dockerState.error).toBeNull(); expect(dockerState.originGeneration).toBe(1);
  await screen.findByText("No networks found"); noEffects();
});
// TestSpec=TC-WINUI-002; TestSpec=TC-WINUI-007; TestSpec=TC-WINUI-035
// TC002/007/035: a current C/I push does not prove V/N recovered. Exercise
// actual App admission and the real poller rather than setting its error flag.
it("keeps failed resource views unavailable through a partial engine event", async () => {
  seam.publications = true; uiState.currentPage = "help"; render(App);
  await waitFor(() => expect(seam.handlers.has("docker-state-updated")).toBe(true));
  await waitFor(() => expect(seam.networks).toHaveBeenCalledTimes(1));
  seam.networks.mockRejectedValueOnce(new Error("current network failure"));
  uiState.currentPage = "networks"; await tick();
  await waitFor(() => expect(dockerState.error).toBeTruthy());
  const error = dockerState.error;
  seam.handlers.get("docker-state-updated")!({ payload: { containers: [], images: [], origin_generation: 1 } });
  await tick();
  expect(dockerState.error).toBe(error);
  expect(screen.queryByText("No networks found")).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent(error!);
  for (const read of [seam.containers, seam.images, seam.networks, seam.volumes]) read.mockResolvedValue([]);
  await (await import("./lib/dataPoller")).refetchAllResources(); await tick();
  expect(dockerState.error).toBeNull();
  await screen.findByText("No networks found"); noEffects();
});

it("does not acquire or present Terminal Docker targets while the host is unknown", async () => {
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function (this: HTMLDialogElement) { this.setAttribute("open", ""); });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (this: HTMLDialogElement) { this.removeAttribute("open"); });
  capabilitiesState.platform = null; uiState.currentPage = "terminal";
  render(App); await tick();
  await fireEvent.click(screen.getByRole("button", { name: "New Session" }));
  expect(seam.containers).not.toHaveBeenCalled();
  expect(screen.getByRole("alert")).toHaveTextContent(/checking/i);
  expect(screen.getByRole("dialog").querySelectorAll(".target-button")).toHaveLength(0);
  noEffects();
});
it("keeps Terminal target-preflight failure visible on a ready Windows engine", async () => {
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function (this: HTMLDialogElement) { this.setAttribute("open", ""); });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (this: HTMLDialogElement) { this.removeAttribute("open"); });
  uiState.currentPage = "terminal";
  dockerState.containers = [container, { ...container, Id: "c".repeat(64), Names: "other" }];
  seam.terminalTarget.mockRejectedValueOnce(new Error("fixture target changed"));
  render(App); await fireEvent.click(screen.getAllByRole("button", { name: "New Session" })[0]);
  await fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: /^fixture/ }));
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("fixture target changed"));
  expect(seam.terminalTarget).toHaveBeenCalledTimes(1); noEffects();
});
it.each((["macos", "linux"] as const).flatMap(os => ["containers", "images", "networks", "volumes"].map(page => ({ os, page }))))("keeps the existing $os $page refresh path", async ({ os, page }) => {
  capabilitiesState.platform!.os = os; capabilitiesState.items.push({ id: "colima", name: "Colima", state: "running" });
  resourceFixtures(); uiState.currentPage = page; render(App); await tick();
  const calls = [seam.containers, seam.images, seam.networks, seam.volumes].map(fn => fn.mock.calls.length);
  const target = ["containers", "images", "networks", "volumes"].indexOf(page);
  expect(calls).toEqual(calls.map((_, i) => i === target ? 1 : 0));
  noEffects();
});

// BR06 / TC018: only the currently mounted destination may consume a new
// navigation request, even when an earlier detached page finishes its read.
it("keeps a new Containers focus and Logs request for the live page sharing the same read", async () => {
  seam.publications = true; uiState.currentPage = "help"; render(App);
  await waitFor(() => expect(seam.handlers.has("docker-reconnected")).toBe(true));
  const poller = await import("./lib/dataPoller"); await poller.refetchAllResources();
  let finish!: (rows: DockerContainer[]) => void;
  seam.containers.mockReturnValueOnce(new Promise<DockerContainer[]>((yes) => { finish = yes; }));
  const before = seam.containers.mock.calls.length;
  uiState.currentPage = "containers"; await tick();
  await waitFor(() => expect(seam.containers).toHaveBeenCalledTimes(before + 1));
  uiState.currentPage = "help"; await tick();
  uiState.focusResource = { page: "containers", id: container.Id };
  uiState.pendingContainerLogs = { containerId: container.Id, engine: "original-engine" };
  uiState.currentPage = "containers"; await tick();
  expect(uiState.focusResource).toEqual({ page: "containers", id: container.Id });
  finish([container]);
  await screen.findByText("fresh container logs");
  expect(uiState.focusResource).toBeNull(); expect(uiState.pendingContainerLogs).toBeNull();
  expect(seam.logs).toHaveBeenCalledTimes(1); noEffects();
});
it.each(["topology", "compose"])("keeps new $page focus until the currently mounted destination has fresh data", async (page) => {
  capabilitiesState.items.push({ id: "docker-compose", name: "Compose", state: "running" });
  const node = { id: `container:${container.Id}`, kind: "container", label: "fresh-focus-node", status: "running", meta: {} };
  const project = { Name: "fresh-focus-project", Status: "running(1)", ConfigFiles: "C:\\資料\\compose.yaml" };
  const rows = page === "topology" ? { nodes: [node], edges: [], warnings: [] } : [project];
  const read = page === "topology" ? seam.topology : seam.projects;
  let old!: (value: unknown) => void; let current!: (value: unknown) => void;
  read.mockReturnValueOnce(new Promise((yes) => { old = yes; }));
  read.mockReturnValueOnce(new Promise((yes) => { current = yes; }));
  uiState.currentPage = page; render(App);
  await waitFor(() => expect(read).toHaveBeenCalledTimes(1));
  uiState.currentPage = "help"; await tick();
  const request = { page, id: page === "topology" ? node.id : project.Name };
  uiState.focusResource = request; uiState.currentPage = page; await tick();
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
  old(rows); await tick(); await tick();
  expect(uiState.focusResource).toEqual(request);
  expect(screen.queryByRole("button", { name: /Open in Containers/i })).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: project.Name })).not.toBeInTheDocument();
  current(rows);
  if (page === "topology") await screen.findByRole("button", { name: /Open in Containers/i });
  else await screen.findByRole("heading", { name: project.Name });
  expect(uiState.focusResource).toBeNull(); expect(seam.diagnose).not.toHaveBeenCalled(); noEffects();
});

// TestSpec=TC-WINUI-002; TestSpec=TC-WINUI-007; TestSpec=TC-WINUI-018; TestSpec=TC-WINUI-034; TestSpec=TC-WINUI-035
it("keeps cold resource views loading until the first complete current snapshot", async () => {
  seam.publications = true; uiState.currentPage = "networks";
  resourceState.loadedGeneration = coldResourceGeneration;
  dockerState.originGeneration = null;
  let finish!: (rows: typeof network[]) => void;
  seam.networks.mockReturnValueOnce(new Promise<typeof network[]>((yes) => { finish = yes; }));
  render(App);
  await waitFor(() => expect(seam.handlers.has("docker-reconnected")).toBe(true));
  await waitFor(() => expect(seam.networks).toHaveBeenCalledTimes(1));
  const pending = (await import("./lib/dataPoller")).refetchAllResources();
  seam.handlers.get("docker-state-updated")!({ payload: { containers: [], images: [], origin_generation: 1 } });
  await tick();
  expect(dockerState.originGeneration).toBe(1); expect(dockerState.error).toBeNull();
  expect(screen.getByRole("status")).toHaveTextContent("Loading resources from the active engine");
  expect(screen.queryByText("No networks found")).not.toBeInTheDocument();
  expect(screen.queryByPlaceholderText(/Search networks/i)).not.toBeInTheDocument();
  finish([]); await pending;
  await screen.findByText("No networks found");
  expect(resourceState.loadedGeneration).toBe(1); noEffects();
});

it.each(["networks", "volumes"].flatMap(page => ["checked", "inspection"].map(intent => ({ page, intent }))))("restores $page $intent only after complete recovered membership", async ({ page, intent }) => {
  resourceFixtures(); seam.publications = true; uiState.currentPage = page;
  const inspect = page === "networks" ? seam.inspectNetwork : seam.inspectVolume;
  inspect.mockResolvedValueOnce("old-held-inspection").mockResolvedValue("fresh-held-inspection");
  render(App);
  await waitFor(() => expect(seam.handlers.has("docker-reconnected")).toBe(true));
  const poller = await import("./lib/dataPoller"); await poller.refetchAllResources();
  await type(/Search (networks|volumes)/i, "fixture");
  if (intent === "checked") { await fireEvent.click(lastCheckbox()); expect(lastCheckbox()).toBeChecked(); }
  else { await openInspection(page); await screen.findByText("old-held-inspection"); }
  seam.handlers.get("docker-connection-lost")!({ payload: {} }); await tick();
  expect(screen.getByRole("heading", { name: /engine unavailable/i })).toBeInTheDocument();
  expect(resourceState.loadedGeneration).toBeNull();
  const read = page === "networks" ? seam.networks : seam.volumes;
  let finish!: (rows: unknown[]) => void;
  read.mockReturnValueOnce(new Promise<unknown[]>((yes) => { finish = yes; }));
  const before = read.mock.calls.length;
  seam.connection.mockResolvedValue({ ...engineConnectionState.snapshot!, active: { ...engineConnectionState.snapshot!.active, ready: true, generation: 2 } });
  seam.handlers.get("docker-reconnected")!({ payload: {} });
  await waitFor(() => expect(read).toHaveBeenCalledTimes(before + 1));
  const pending = poller.refetchAllResources();
  seam.handlers.get("docker-state-updated")!({ payload: { containers: [], images: [], origin_generation: 2 } });
  await tick();
  expect(dockerState.originGeneration).toBe(2); expect(dockerState.error).toBeNull();
  expect(screen.getByRole("status")).toHaveTextContent("Loading resources from the active engine");
  expect(screen.queryByPlaceholderText(/Search (networks|volumes)/i)).not.toBeInTheDocument();
  expect(inspect).toHaveBeenCalledTimes(intent === "inspection" ? 1 : 0);
  finish(page === "networks" ? [network] : [volume]); await pending;
  await screen.findByPlaceholderText(/Search (networks|volumes)/i);
  expectInput(/Search (networks|volumes)/i, "fixture");
  if (intent === "checked") expect(lastCheckbox()).toBeChecked();
  else { await screen.findByText("fresh-held-inspection"); expect(inspect).toHaveBeenCalledTimes(2); }
  expect(resourceState.loadedGeneration).toBe(2); noEffects();
});
