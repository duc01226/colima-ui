import { afterEach, beforeEach, expect, it, vi } from "vitest";
const api = vi.hoisted(() => ({ resolveApiBase: vi.fn(), getApiToken: vi.fn().mockResolvedValue("fixture-token") }));
vi.mock("./api", () => api);
vi.mock("./api/client", () => ({ ...api, call: vi.fn() }));
vi.mock("./env", () => ({ isRunningInTauri: () => false }));
let opened = 0;
class Source { constructor() { opened++; } addEventListener() {} close() {} }
beforeEach(() => { vi.resetModules(); vi.clearAllMocks(); vi.useFakeTimers(); opened = 0; vi.stubGlobal("EventSource", Source); });
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
// TC009/014/018: unbound native API or failed browser discovery is a gap, never
// another server or an unhandled promise; retry belongs to the subscription.
it.each(["metrics", "transfer"])("%s bounds discovery retry and cancels the pending timer", async (kind) => {
  api.resolveApiBase.mockRejectedValue(new Error("not bound"));
  const error = vi.fn();
  const stop = kind === "metrics"
    ? (await import("./api/metrics")).subscribeMetrics({ onError: error })
    : (await import("./transferEvents")).subscribeTransfer({ onDesync: error });
  await vi.advanceTimersByTimeAsync(0);
  expect(error).toHaveBeenCalledTimes(1); expect(opened).toBe(0); expect(vi.getTimerCount()).toBe(1);
  await vi.advanceTimersByTimeAsync(2000); expect(api.resolveApiBase).toHaveBeenCalledTimes(2);
  expect(vi.getTimerCount()).toBe(1);
  stop(); expect(vi.getTimerCount()).toBe(0);
});
it.each(["metrics", "transfer"])("%s does not open a socket when discovery finishes after unsubscribe", async (kind) => {
  let finish!: (base: string) => void;
  api.resolveApiBase.mockReturnValue(new Promise<string>((resolve) => { finish = resolve; }));
  const stop = kind === "metrics" ? (await import("./api/metrics")).subscribeMetrics({}) : (await import("./transferEvents")).subscribeTransfer({});
  await vi.advanceTimersByTimeAsync(0); stop(); finish("http://127.0.0.1:11425"); await vi.advanceTimersByTimeAsync(0);
  expect(opened).toBe(0); expect(vi.getTimerCount()).toBe(0);
});
