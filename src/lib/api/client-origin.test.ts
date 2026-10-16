import { afterEach, beforeEach, expect, it, vi } from "vitest";
const seam = vi.hoisted(() => ({ native: true, invoke: vi.fn() }));
vi.mock("../env", () => ({ isRunningInTauri: () => seam.native }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: seam.invoke }));
beforeEach(() => { vi.resetModules(); vi.clearAllMocks(); seam.native = true; vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true })); });
afterEach(() => vi.unstubAllGlobals());
// TC014/016: native credentials must never be sent to a health-discovered sibling process.
it("uses only this app's IPC port and never probes another server", async () => {
  seam.invoke.mockResolvedValueOnce(11426);
  const client = await import("./client");
  expect(await client.resolveApiBase()).toBe("http://127.0.0.1:11426");
  expect(seam.invoke).toHaveBeenCalledWith("api_port"); expect(fetch).not.toHaveBeenCalled();
});
it.each([null, "11420", 11419, 11430, 11420.5])("refuses invalid native port %s without any health fallback", async (port) => {
  seam.invoke.mockResolvedValueOnce(port);
  const client = await import("./client"); await expect(client.resolveApiBase()).rejects.toThrow("not ready");
  expect(fetch).not.toHaveBeenCalled();
});
it("refuses an unbound native instance without reusing a previously returned port", async () => {
  seam.invoke.mockResolvedValueOnce(11423).mockRejectedValueOnce(new Error("unbound"));
  const client = await import("./client"); await client.resolveApiBase();
  await expect(client.resolveApiBase()).rejects.toThrow("unbound"); expect(fetch).not.toHaveBeenCalled();
});
it("preserves browser development health discovery", async () => {
  seam.native = false;
  const client = await import("./client"); expect(await client.resolveApiBase()).toBe("http://127.0.0.1:11420");
  expect(fetch).toHaveBeenCalledWith("http://127.0.0.1:11420/api/health", expect.anything()); expect(seam.invoke).not.toHaveBeenCalled();
});
