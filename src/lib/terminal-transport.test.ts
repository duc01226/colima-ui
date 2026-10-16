import { beforeEach, expect, it, vi } from "vitest";
import { containerTerminalTarget, openTerminal, sessionLabel, exitHint } from "./terminal-transport";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  Channel: class { onmessage = (_text: string) => {}; },
}));
beforeEach(() => invoke.mockReset());

it("carries the backend origin and stable container ID, with a display-only name", async () => {
  invoke.mockResolvedValue({ kind: "containerExec", containerId: "abcdef123456", engine: "rancher-desktop" });
  const target = await containerTerminalTarget("abcdef123456", "数据库");
  expect(invoke).toHaveBeenCalledWith("terminal_target", { containerId: "abcdef123456" });
  expect(target).toEqual({ kind: "containerExec", containerId: "abcdef123456", engine: "rancher-desktop", containerName: "数据库" });
  expect(sessionLabel(target)).toContain("rancher-desktop");
  expect(sessionLabel(target)).toContain("数据库");
});

it("refuses a missing, wrong-kind or different-container target instead of opening a fallback", async () => {
  for (const target of [
    { kind: "colima", profile: "default" },
    { kind: "containerExec", containerId: "other", engine: "desktop-linux" },
    { kind: "containerExec", containerId: "abcdef123456", engine: "" },
  ]) {
    invoke.mockResolvedValueOnce(target);
    await expect(containerTerminalTarget("abcdef123456", "work")).rejects.toThrow("Invalid container terminal target");
  }
  expect(invoke.mock.calls.every(([command]) => command === "terminal_target")).toBe(true);
});

it("uses only the existing IPC lifecycle and forwards input and resize literally", async () => {
  invoke.mockResolvedValue(undefined);
  const kind = { kind: "containerExec", containerId: "abcdef123456", engine: "desktop-linux" } as const;
  const handle = await openTerminal("session-a", kind, () => {});
  await handle.write("echo 你好\r\u0003");
  await handle.resize(40, 120);
  await handle.close();
  expect(invoke.mock.calls.map(([command]) => command)).toEqual(["terminal_create", "terminal_write", "terminal_resize", "terminal_close"]);
  expect(invoke.mock.calls[0][1]).toMatchObject({ sessionId: "session-a", kind });
  expect(invoke.mock.calls[1][1]).toEqual({ sessionId: "session-a", data: "echo 你好\r\u0003" });
  expect(invoke.mock.calls[2][1]).toEqual({ sessionId: "session-a", rows: 40, cols: 120 });
  expect(exitHint(kind, 127)).toContain("no usable shell");
  expect(exitHint(kind, 0)).toBeNull();
});

it("registers the captured-target IPC alongside the existing lifecycle commands", async () => {
  const backend = (await import("../../src-tauri/src/lib.rs?raw")).default;
  expect(backend).toContain("terminal::terminal_target,");
  for (const command of ["terminal_create", "terminal_write", "terminal_resize", "terminal_poll_exit", "terminal_close"]) {
    expect(backend).toContain(`terminal::${command},`);
  }
});
