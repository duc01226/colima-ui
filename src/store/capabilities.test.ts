import { beforeEach, expect, it, vi } from "vitest";
const api = vi.hoisted(() => ({ getCapabilities: vi.fn(), getPlatform: vi.fn() }));
vi.mock("../lib/api", () => ({ systemApi: api }));
beforeEach(() => { vi.resetModules(); vi.clearAllMocks(); });
const ready = [{ id: "colima", name: "Colima", state: "unsupported" }, { id: "docker", name: "Docker", state: "running" }];
// TC002/015/018: Docker-ready is independent of Colima; unknown never means empty success.
it("allows Windows Docker resources while denying VM and Kubernetes acquisition", async () => {
  api.getCapabilities.mockResolvedValue(ready); api.getPlatform.mockResolvedValue({ os: "windows" });
  const caps = await import("./capabilities.svelte"); await caps.loadCapabilities();
  expect(caps.blockingCapability("colima", "docker")).toBeUndefined();
  expect(caps.supportsHostVms()).toBe(false); expect(caps.supportsKubernetes()).toBe(false);
  for (const page of ["instances", "linux-vms", "models", "kubernetes", "xray", "cluster-topology"]) expect(caps.pageAvailable(page)).toBe(false);
  for (const page of ["containers", "images", "compose", "terminal", "settings", "help"]) expect(caps.pageAvailable(page)).toBe(true);
});
it("coalesces detection and preserves facts as stale after failure", async () => {
  api.getCapabilities.mockResolvedValue(ready); api.getPlatform.mockResolvedValue({ os: "windows" });
  const caps = await import("./capabilities.svelte"); await Promise.all([caps.loadCapabilities(), caps.loadCapabilities()]);
  expect(api.getCapabilities).toHaveBeenCalledTimes(1);
  api.getCapabilities.mockRejectedValueOnce(new Error("down")); await caps.loadCapabilities();
  expect(caps.capabilityState("docker")).toBe("running");
  expect(caps.blockingCapability("docker")?.state).toBe("unknown");
  expect(caps.capabilitiesState.failed).toBe(true);
});
it("gives provider-neutral stopped-engine recovery and preserves POSIX capability rules", async () => {
  const caps = await import("./capabilities.svelte");
  caps.capabilitiesState.platform = { os: "windows" } as never;
  const notice = caps.capabilityNotice({ id: "docker", name: "Docker", state: "installed_not_running" });
  expect(notice.text).toContain("Docker Desktop"); expect(notice.text).toContain("Moby"); expect(notice.text).not.toContain("Colima instance");
  caps.capabilitiesState.platform = { os: "macos" } as never; caps.capabilitiesState.items = ready as never;
  expect(caps.supportsHostVms()).toBe(true); expect(caps.pageAvailable("kubernetes")).toBe(true);
  expect(caps.blockingCapability("colima", "docker")?.id).toBe("colima");
});

it("treats absent capability data as unknown and refuses unsupported acquisition before host detection", async () => {
  const caps = await import("./capabilities.svelte");
  expect(caps.supportsHostVms()).toBe(false); expect(caps.supportsKubernetes()).toBe(false);
  expect(caps.blockingCapability("docker")?.state).toBe("unknown");
});
