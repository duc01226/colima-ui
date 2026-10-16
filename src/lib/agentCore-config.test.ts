import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ChatMessage } from "./api";
import brief from "../../src-tauri/resources/windows_docker_assistant.md?raw";
import { parseAiTools } from "./aiToolParser";

const seam = vi.hoisted(() => ({
  os: "windows" as "windows" | "macos" | "linux", invoke: vi.fn(), chat: vi.fn(), sandbox: vi.fn(),
  containers: vi.fn().mockResolvedValue([]), projects: vi.fn().mockResolvedValue([]), stop: vi.fn().mockResolvedValue(undefined), up: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: seam.invoke }));
vi.mock("./llmProviders", () => ({ chatStream: seam.chat }));
vi.mock("./api", () => ({
  colimaApi: {}, dockerApi: { listContainers: seam.containers, stopContainer: seam.stop }, composeApi: { list: seam.projects, up: seam.up }, k8sApi: {}, kindApi: {}, limaApi: {}, modelsApi: {}, volumesApi: {}, networksApi: {}, sysMethods: {}, sandboxApi: {}, aiApi: {},
  knowledgeBankApi: { query: vi.fn().mockResolvedValue({}), searchMemory: vi.fn().mockResolvedValue([]) },
}));
vi.mock("./api/sandbox", () => ({ sandboxApi: { executeApproved: seam.sandbox } }));
vi.mock("./settingsStore.svelte", () => ({ getAppSetting: (_key: string, fallback: string) => fallback, setAppSetting: vi.fn() }));
vi.mock("./presetStateManager", () => ({ BUILT_IN_PRESETS: [] }));
vi.mock("../store.svelte", () => ({ openTerminalSession: vi.fn() }));
vi.mock("../store/capabilities.svelte", () => ({ capabilitiesState: { get platform() { return { os: seam.os }; } }, isDockerOnlyHost: () => seam.os === "windows", supportsHostVms: () => seam.os !== "windows", supportsKubernetes: () => seam.os !== "windows" }));

beforeEach(() => {
  vi.clearAllMocks();
  seam.os = "windows";
  vi.stubGlobal("isTauri", undefined);
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
});
afterEach(() => { Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); vi.unstubAllGlobals(); });

async function queryStatus() {
  const histories: ChatMessage[][] = [];
  seam.chat.mockImplementation(async (_provider: string, _model: string, key: string, history: ChatMessage[], _endpoint: string, chunk: (text: string) => void) => {
    // The selected provider still receives its credential for authentication;
    // the prohibition concerns model content, not that transport argument.
    expect(key).toBe("fixture-selected-auth");
    histories.push(structuredClone(history));
    chunk(histories.length === 1 ? "[QUERY: ai-config-status]" : "Finished");
  });
  const callbacks = { onStatus: vi.fn(), onMessage: vi.fn(), onMessageUpdate: vi.fn(), onMessageDelete: vi.fn(), onApprovalRequired: vi.fn(), onNavigate: vi.fn(), onTimerSchedule: vi.fn(), onCronSchedule: vi.fn(), onScheduleCancel: vi.fn() };
  // Actual native config handler + merged registry + parser + sender. Only
  // native I/O and the model boundary are substituted; no paid model call.
  await (await import("./agentCore")).runAgent("Check AI configuration", { provider: "openai", model: "fixture-model", apiKey: "fixture-selected-auth", endpoint: "" }, callbacks, [], brief, []);
  expect(histories).toHaveLength(2);
  expect(histories[0][0].content).toContain(brief);
  expect(histories[0][0].content).toContain('"name":"ai-config-status"');
  expect(callbacks.onApprovalRequired).not.toHaveBeenCalled();
  expect(seam.invoke).toHaveBeenCalledExactlyOnceWith("get_all_settings");
  const result = histories[1][histories[1].length - 1].content.match(/### Query: ai-config-status\n([^\n]+)/)?.[1];
  expect(result).toBeDefined();
  return { histories, status: JSON.parse(result!) };
}

// TestSpec=TC-WINUI-016 — partial configuration privacy contract; full native case pending.
// BR05 / TC-WINUI-016: stored active/inactive credentials and arbitrary private
// settings must never become model instructions, tool results or history.
it.each(["scoped", "legacy", "missing", "unknown-provider", "failure"])("minimizes the real SAFE config query through outgoing history: %s", async (variant) => {
  const settings: Record<string, string> = {
    ai_provider: "openai", private_setting: "fixture-unrelated-secret",
    "ai_api_key.anthropic": "fixture-inactive-key",
    "ai_endpoint.anthropic": "https://inactive-user:inactive-password@example.invalid",
  };
  if (variant === "unknown-provider") settings.ai_provider = "fixture-private-provider-text";
  if (variant === "scoped" || variant === "legacy") {
    const suffix = variant === "scoped" ? ".openai" : "";
    settings[`ai_api_key${suffix}`] = "fixture-active-key";
    settings[`ai_endpoint${suffix}`] = "https://private-user:private-password@example.invalid/?token=fixture-url-token";
    settings[`ai_model${suffix}`] = "fixture-private-model-text";
  }
  if (variant === "failure") seam.invoke.mockRejectedValueOnce(new Error("fixture-error-secret C:\\private\\settings https://user:password@example.invalid"));
  else seam.invoke.mockResolvedValueOnce(settings);
  const { histories, status } = await queryStatus();
  const sent = JSON.stringify(histories);
  for (const value of Object.values(settings).filter(value => value !== "openai")) expect(sent).not.toContain(value);
  expect(sent).not.toContain("fixture-selected-auth");
  expect(sent).not.toContain("fixture-error-secret");
  expect(status).toEqual(variant === "failure"
    ? { status: "unavailable", reason: "AI configuration could not be read; current status is unknown." }
    : { status: "available", provider: variant === "unknown-provider" ? "unknown" : "openai", modelConfigured: ["scoped", "legacy"].includes(variant), keyConfigured: ["scoped", "legacy"].includes(variant), endpointConfigured: ["scoped", "legacy"].includes(variant) });
});

// TC-WINUI-016: exercise the actual packaged instructions against the real
// parser, so adding an unimplemented tool to the brief breaks this contract.
it("advertises only tool syntax the actual Windows sender recognizes", () => {
  const commands = [...brief.matchAll(/`(\[[^\n`]+\])`/g)].map(match => match[1]);
  expect(commands.length).toBeGreaterThan(5);
  for (const command of commands) expect(parseAiTools(command).hasTools, command).toBe(true);
  expect(parseAiTools("[QUERY: ai-config-status]").queries).toHaveLength(1);
  expect(parseAiTools('[EVENT_APPROVE: stop-container | {"id":"fixture"}]').eventApprovals).toHaveLength(1);
  expect(parseAiTools("[READ_REFERENCE: skills/docker/SKILL.md]").readReferences).toHaveLength(1);
  expect(parseAiTools("[UNKNOWN: fixture]").hasTools).toBe(false);
});

// TestSpec=TC-WINUI-015; TestSpec=TC-WINUI-016 — partial real tool contract guards.
// BR05/06 / TC015/016: actual registry definitions, parser and sender must
// agree with executable capabilities; a fake registry/token-only test cannot.
it.each(["build-image", "compose-build", "compose-pull"])("refuses the actual unsupported Windows event %s before approval or effect", async (name) => {
  const bus = await import("./aiEventBus");
  expect(bus.listEvents().map(event => event.name)).not.toContain(name);
  expect(bus.getCategory(name)).toBeNull();
  await expect(bus.executeEvent(name, {})).rejects.toThrow("Unavailable on this host");
  const histories: ChatMessage[][] = [];
  seam.chat.mockImplementation(async (_provider: string, _model: string, _key: string, history: ChatMessage[], _endpoint: string, chunk: (text: string) => void) => {
    histories.push(structuredClone(history));
    chunk(histories.length === 1 ? `[EVENT_APPROVE: ${name} | {}]` : "Finished");
  });
  const callbacks = { onStatus: vi.fn(), onMessage: vi.fn(), onMessageUpdate: vi.fn(), onMessageDelete: vi.fn(), onApprovalRequired: vi.fn().mockResolvedValue(true), onNavigate: vi.fn(), onTimerSchedule: vi.fn(), onCronSchedule: vi.fn(), onScheduleCancel: vi.fn() };
  await (await import("./agentCore")).runAgent("Perform this action", { provider: "openai", model: "fixture", apiKey: "fixture-selected-auth", endpoint: "" }, callbacks, [], brief, []);
  expect(histories).toHaveLength(2);
  for (const unsupported of ["build-image", "compose-build", "compose-pull"]) expect(histories[0][0].content).not.toContain(`"name":"${unsupported}"`);
  expect(histories[1][histories[1].length - 1].content).toContain(`Event unavailable: ${name}. No action was requested.`);
  expect(callbacks.onApprovalRequired).not.toHaveBeenCalled();
  for (const effect of [seam.invoke, seam.sandbox, seam.stop, seam.up]) expect(effect).not.toHaveBeenCalled();
});

it("preserves actual supported Windows Docker and Compose operations", async () => {
  const bus = await import("./aiEventBus");
  for (const name of ["list-containers", "compose-list"]) expect(bus.getCategory(name)).toBe("SAFE");
  for (const name of ["stop-container", "compose-up"]) expect(bus.getCategory(name)).toBe("NORMAL");
  expect(JSON.parse(await bus.executeEvent("list-containers", {}))).toEqual([]);
  expect(JSON.parse(await bus.executeEvent("compose-list", {}))).toEqual([]);
  await bus.executeEvent("stop-container", { id: "owned-fixture" });
  await bus.executeEvent("compose-up", { dir: "owned-project" });
  expect(seam.containers).toHaveBeenCalledTimes(1); expect(seam.projects).toHaveBeenCalledTimes(1);
  expect(seam.stop).toHaveBeenCalledExactlyOnceWith("owned-fixture");
  expect(seam.up).toHaveBeenCalledExactlyOnceWith("owned-project", undefined);
  expect(seam.sandbox).not.toHaveBeenCalled();
});

it.each(["macos", "linux"] as const)("preserves the actual %s event catalog and categories", async (os) => {
  seam.os = os;
  const bus = await import("./aiEventBus");
  for (const name of ["build-image", "compose-build", "compose-pull"]) {
    expect(bus.listEvents().map(event => event.name)).toContain(name);
    expect(bus.getCategory(name)).toBe("NORMAL");
  }
  expect(seam.sandbox).not.toHaveBeenCalled();
});
