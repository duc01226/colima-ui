import { beforeEach, expect, it, vi } from "vitest";
import type { ChatMessage } from "./api";
const seam = vi.hoisted(() => ({ host: "windows" as "windows" | "macos" | "linux" | "unknown", chat: vi.fn(), instances: vi.fn(), k8s: vi.fn(), diagnostics: vi.fn(), containers: vi.fn(), compose: vi.fn() }));
vi.mock("./api", () => ({
  colimaApi: { listInstances: seam.instances }, dockerApi: { listContainers: seam.containers }, composeApi: { list: seam.compose }, k8sApi: { currentContext: seam.k8s },
  sysMethods: { hostSpecs: vi.fn().mockResolvedValue({ cpus: 4 }) }, knowledgeBankApi: { query: vi.fn().mockResolvedValue({}), searchMemory: vi.fn().mockResolvedValue([]), collectDiagnosticLogs: seam.diagnostics }, sandboxApi: {}, aiApi: {},
}));
vi.mock("./llmProviders", () => ({ chatStream: seam.chat }));
vi.mock("./aiProviderConfig", () => ({ findProvider: () => ({ needsKey: false, needsEndpoint: false }) }));
vi.mock("./presetStateManager", () => ({ BUILT_IN_PRESETS: [{ id: "vm-fixture" }] }));
vi.mock("../store/capabilities.svelte", () => ({ capabilitiesState: { get platform() { return seam.host === "unknown" ? null : { os: seam.host }; } }, isDockerOnlyHost: () => seam.host === "windows", supportsHostVms: () => seam.host !== "windows" && seam.host !== "unknown", supportsKubernetes: () => seam.host !== "windows" && seam.host !== "unknown" }));
vi.mock("./aiEventBus", () => ({ getCategory: () => seam.host === "windows" || seam.host === "unknown" ? null : "NORMAL", executeEvent: vi.fn(), listEvents: () => [{ name: "list-containers", category: "SAFE", description: "Docker containers" }] }));
beforeEach(() => {
  vi.resetModules(); vi.clearAllMocks(); seam.host = "windows"; seam.instances.mockResolvedValue([]); seam.k8s.mockResolvedValue("legacy-context"); seam.diagnostics.mockResolvedValue("legacy diagnostics"); seam.containers.mockResolvedValue([]); seam.compose.mockResolvedValue([]);
});
async function exercise() {
  const histories: ChatMessage[][] = [];
  seam.chat.mockImplementation(async (_provider: string, _model: string, _key: string, history: ChatMessage[], _endpoint: string, chunk: (text: string) => void) => {
    histories.push(structuredClone(history));
    chunk(histories.length === 1 ? '[QUERY_APP_STATE]\n[EVENT_APPROVE: start-instance | {"profile":"default"}]\n[DIAGNOSE]' : "Finished");
  });
  const callbacks = { onStatus: vi.fn(), onMessage: vi.fn(), onMessageUpdate: vi.fn(), onMessageDelete: vi.fn(), onApprovalRequired: vi.fn().mockResolvedValue(false), onNavigate: vi.fn(), onTimerSchedule: vi.fn(), onCronSchedule: vi.fn(), onScheduleCancel: vi.fn() };
  await (await import("./agentCore")).runAgent("Inspect my containers", { provider: "fixture", model: "fixture", apiKey: "", endpoint: "" }, callbacks, [], "Fixture app instructions", [{ id: "custom-vm" }]);
  return { histories, callbacks };
}
// TC015/016: inspect actual outgoing model context and tool trajectory at the
// model seam; no live model call, and no change to the bounded reasoning loop.
it("sends filtered Windows tools and never acquires unsupported legacy state or approval", async () => {
  const { histories, callbacks } = await exercise();
  expect(histories).toHaveLength(2);
  expect(histories[0][0].content).toContain('"name":"list-containers"');
  expect(histories[0][0].content).not.toContain("start-instance");
  expect(seam.instances).not.toHaveBeenCalled(); expect(seam.k8s).not.toHaveBeenCalled(); expect(seam.diagnostics).not.toHaveBeenCalled();
  expect(seam.containers).toHaveBeenCalledTimes(1); expect(seam.compose).toHaveBeenCalledTimes(1);
  expect(callbacks.onApprovalRequired).not.toHaveBeenCalled();
  const results = histories[1][histories[1].length - 1]?.content ?? "";
  expect(results).toContain("unavailable on this host"); expect(results).toContain("No action was requested"); expect(results).not.toContain("vm-fixture");
});
it.each(["macos", "linux"] as const)("preserves %s context and supported legacy state/approval behavior", async (host) => {
  seam.host = host;
  const { histories, callbacks } = await exercise();
  expect(histories[0][0].content).toBe("Fixture app instructions");
  expect(seam.instances).toHaveBeenCalledTimes(1); expect(seam.k8s).toHaveBeenCalledTimes(1); expect(seam.diagnostics).toHaveBeenCalledTimes(1);
  expect(callbacks.onApprovalRequired).toHaveBeenCalledTimes(1);
  expect(histories[1][histories[1].length - 1]?.content).toContain("vm-fixture");
});

// BR06 / TC002,016: tool failures are unknown, not a healthy empty engine.
it.each([
  { containersFailed: false, composeFailed: false },
  { containersFailed: true, composeFailed: false },
  { containersFailed: false, composeFailed: true },
  { containersFailed: true, composeFailed: true },
].flatMap(outcome => (["windows", "unknown", "macos", "linux"] as const).map(host => ({ ...outcome, host }))))("attributes failed supported reads without exposing rejection details: %j", async ({ containersFailed, composeFailed, host }) => {
  seam.host = host;
  const privateFailure = new Error("Bearer fixture-secret C:\\Users\\private");
  if (containersFailed) seam.containers.mockRejectedValue(privateFailure);
  if (composeFailed) seam.compose.mockRejectedValue(privateFailure);
  const { histories, callbacks } = await exercise();
  expect(histories).toHaveLength(2);
  const result = histories[1][histories[1].length - 1]?.content ?? "";
  const containers = JSON.parse(result.match(/^Containers \(summary\): (.*)$/m)?.[1] ?? "null");
  const compose = JSON.parse(result.match(/^Compose Projects: (.*)$/m)?.[1] ?? "null");
  expect(containers).toEqual(containersFailed
    ? host === "windows" || host === "unknown" ? { status: "unavailable", reason: "Container query failed; current state is unknown." } : []
    : []);
  expect(compose).toEqual(composeFailed
    ? host === "windows" || host === "unknown" ? { status: "unavailable", reason: "Compose query failed; current state is unknown." } : []
    : []);
  expect(JSON.stringify(histories)).not.toContain("fixture-secret");
  expect(JSON.stringify(histories)).not.toContain("C:\\Users\\private");
  expect(callbacks.onApprovalRequired).toHaveBeenCalledTimes(host === "macos" || host === "linux" ? 1 : 0);
  expect(seam.instances).toHaveBeenCalledTimes(host === "macos" || host === "linux" ? 1 : 0);
  expect(seam.k8s).toHaveBeenCalledTimes(host === "macos" || host === "linux" ? 1 : 0);
  expect(seam.diagnostics).toHaveBeenCalledTimes(host === "macos" || host === "linux" ? 1 : 0);
});
