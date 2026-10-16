import { beforeEach, expect, it, vi } from "vitest";
const host = vi.hoisted(() => ({ windows: true, vm: false, k8s: false, run: vi.fn().mockResolvedValue("executed") }));
vi.mock("../store/capabilities.svelte", () => ({ isDockerOnlyHost: () => host.windows, supportsHostVms: () => host.vm, supportsKubernetes: () => host.k8s }));
// Substitute effect boundary only; exercise actual merged registry/filter/dispatch owner.
vi.mock("./aiEvents/colima", () => ({ colimaRegistry: { "list-instances": { category: "SAFE", description: "VM", handler: host.run } } }));
vi.mock("./aiEvents/k8s", () => ({ k8sRegistry: { "k8s-pods": { category: "SAFE", description: "pods", handler: host.run } } }));
vi.mock("./aiEvents/lima", () => ({ limaRegistry: { "lima-start": { category: "NORMAL", description: "VM start", handler: host.run } } }));
vi.mock("./aiEvents/docker", () => ({ dockerRegistry: { "docker-ps": { category: "SAFE", description: "containers", handler: host.run }, "docker-rm": { category: "DANGEROUS", description: "remove", handler: host.run } } }));
vi.mock("./aiEvents/volumes", () => ({ volumesRegistry: {} }));
vi.mock("./aiEvents/system", () => ({ systemRegistry: {} }));
vi.mock("./aiEvents/compose", () => ({ composeRegistry: {} }));
vi.mock("./aiEvents/config", () => ({ configRegistry: { "model-pull": { category: "NORMAL", description: "model", handler: host.run }, "save-preset": { category: "NORMAL", description: "preset", handler: host.run }, "navigate": { category: "SAFE", description: "UI", handler: host.run } } }));
beforeEach(() => { vi.resetModules(); vi.clearAllMocks(); host.windows = true; host.vm = false; host.k8s = false; });
// TC015/016: deterministic tool-set eval; supported Docker categories stay identical.
it("filters unavailable tools from advertisement, category and direct dispatch", async () => {
  const bus = await import("./aiEventBus");
  const unsupported = ["list-instances", "k8s-pods", "lima-start", "model-pull", "save-preset"];
  expect(bus.listEvents().map((e) => e.name).sort()).toEqual(["docker-ps", "docker-rm", "navigate"]);
  for (const name of unsupported) {
    expect(bus.getCategory(name)).toBeNull();
    await expect(bus.executeEvent(name, {})).rejects.toThrow("Unavailable on this host");
  }
  expect(host.run).not.toHaveBeenCalled();
  expect(bus.getCategory("docker-ps")).toBe("SAFE"); expect(bus.getCategory("docker-rm")).toBe("DANGEROUS");
  expect(await bus.executeEvent("docker-ps", {})).toBe("executed"); expect(host.run).toHaveBeenCalledTimes(1);
});
it("preserves the supported-platform tool set without reclassifying authority", async () => {
  host.windows = false; host.vm = true; host.k8s = true;
  const bus = await import("./aiEventBus");
  expect(bus.listEvents()).toHaveLength(8);
  expect(bus.getCategory("lima-start")).toBe("NORMAL");
  expect(bus.getCategory("docker-rm")).toBe("DANGEROUS");
  await expect(bus.executeEvent("unregistered", {})).rejects.toThrow("Unknown event");
});
