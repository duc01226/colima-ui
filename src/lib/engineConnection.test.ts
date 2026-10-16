import { beforeEach, describe, expect, it, vi } from "vitest";
const api = vi.hoisted(() => ({ get: vi.fn(), check: vi.fn(), save: vi.fn(), restart: vi.fn() }));
vi.mock("./api/system", () => ({ connectionApi: api }));
const status = (context = "desktop-linux") => ({ context, endpoint: "npipe:////./pipe/fixture", engine_id: "original", generation: 1, ready: true, error: null });
const settings = () => ({ active: status(), pending: null, contexts: [], discovery_error: null });
function held<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => { resolve = done; }); return { promise, resolve }; }
beforeEach(() => { vi.resetModules(); vi.clearAllMocks(); api.get.mockResolvedValue(settings()); api.check.mockImplementation(async (context: string) => status(context)); api.save.mockResolvedValue(undefined); api.restart.mockResolvedValue(undefined); });

// TC004/031/035: candidate success cannot retarget or certify the active session.
describe("immutable active engine and pending preference", () => {
  it("checks a candidate and saves only the next launch after persistence succeeds", async () => {
    const state = await import("./engineConnection.svelte");
    await state.loadEngineConnection();
    state.changeEngineDraft("rancher-desktop");
    await state.checkEngineDraft();
    expect(state.engineConnectionState.snapshot?.active.context).toBe("desktop-linux");
    expect(state.engineConnectionState.snapshot?.pending).toBeNull();
    const save = held<void>(); api.save.mockReturnValueOnce(save.promise);
    const saving = state.saveEngineDraft();
    expect(state.engineConnectionState.snapshot?.pending).toBeNull();
    save.resolve(); await saving;
    expect(state.engineConnectionState.snapshot?.pending).toBe("rancher-desktop");
    expect(state.engineConnectionState.snapshot?.active.context).toBe("desktop-linux");
    expect(api.restart).not.toHaveBeenCalled();
  });
  it("preserves the draft and previous preference after rejected persistence", async () => {
    const state = await import("./engineConnection.svelte"); await state.loadEngineConnection();
    state.changeEngineDraft("rancher-desktop"); await state.checkEngineDraft();
    api.save.mockRejectedValueOnce(new Error("fixture write refused"));
    await expect(state.saveEngineDraft()).rejects.toThrow("write refused");
    expect(state.engineConnectionState.draft).toBe("rancher-desktop");
    expect(state.engineConnectionState.snapshot?.pending).toBeNull();
    expect(state.engineConnectionState.snapshot?.active.ready).toBe(true);
    expect(state.engineConnectionState.saving).toBe(false);
  });
  it("cannot use a late check to certify a different selection", async () => {
    const state = await import("./engineConnection.svelte"); await state.loadEngineConnection();
    const check = held<ReturnType<typeof status>>(); api.check.mockReturnValueOnce(check.promise);
    state.changeEngineDraft("candidate-a"); const checking = state.checkEngineDraft();
    state.changeEngineDraft("candidate-b"); check.resolve(status("candidate-a")); await checking;
    expect(state.engineConnectionState.checked).toBeNull();
    await expect(state.saveEngineDraft()).rejects.toThrow("Check this selection");
    expect(api.save).not.toHaveBeenCalled();
  });
  it("does not resurrect ready state from a request started before connection loss", async () => {
    const state = await import("./engineConnection.svelte"); await state.loadEngineConnection();
    const get = held<ReturnType<typeof settings>>(); api.get.mockReturnValueOnce(get.promise);
    const loading = state.loadEngineConnection(); state.markEngineUnavailable();
    get.resolve(settings()); await loading;
    expect(state.engineConnectionState.snapshot?.active.ready).toBe(false);
    expect(state.engineConnectionState.snapshot?.active.generation).toBeNull();
    expect(state.engineConnectionState.loading).toBe(false);
  });
  it("retains a missing saved context and never picks the first supported entry", async () => {
    api.get.mockResolvedValueOnce({ ...settings(), pending: "deleted-context", contexts: [{ name: "rancher-desktop", supported: true, reason: null }] });
    const state = await import("./engineConnection.svelte"); await state.loadEngineConnection();
    expect(state.engineConnectionState.draft).toBe("deleted-context");
    expect(api.check).not.toHaveBeenCalled(); expect(api.save).not.toHaveBeenCalled();
  });
  it("restart requests no implicit transfer cancellation and stays editable on refusal", async () => {
    const state = await import("./engineConnection.svelte");
    api.restart.mockRejectedValueOnce(new Error("transfers active"));
    await expect(state.restartForEngine()).rejects.toThrow("transfers active");
    expect(api.restart).toHaveBeenCalledWith(false);
    expect(state.engineConnectionState.restarting).toBe(false);
  });
});
