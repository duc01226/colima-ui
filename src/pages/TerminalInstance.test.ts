import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, render, waitFor, screen } from "@testing-library/svelte";
import type { TerminalHandle } from "../lib/terminal-transport";

const { openTerminal, terminal } = vi.hoisted(() => ({
  openTerminal: vi.fn(),
  terminal: { input: null as null | ((text: string) => void), write: vi.fn(), writeln: vi.fn(), dispose: vi.fn(), focus: vi.fn(), options: {} },
}));
vi.mock("../lib/terminal-transport", async () => ({
  ...await vi.importActual<typeof import("../lib/terminal-transport")>("../lib/terminal-transport"), openTerminal,
}));
vi.mock("@xterm/xterm", () => ({ Terminal: class {
  options = terminal.options; rows = 24; cols = 80;
  write = terminal.write; writeln = terminal.writeln; dispose = terminal.dispose; focus = terminal.focus;
  open() {} loadAddon() {} attachCustomKeyEventHandler() {}
  onData(callback: (text: string) => void) { terminal.input = callback; }
} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
import TerminalInstance from "./TerminalInstance.svelte";

const kind = { kind: "containerExec", containerId: "abcdef123456", engine: "desktop-linux", containerName: "work" } as const;
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function handle(): TerminalHandle {
  return { write: vi.fn().mockResolvedValue(undefined), resize: vi.fn().mockResolvedValue(undefined), pollExit: vi.fn().mockResolvedValue(null), close: vi.fn().mockResolvedValue(undefined) };
}
beforeEach(() => {
  vi.clearAllMocks(); terminal.input = null; terminal.options = {};
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

it("closes an acquisition completed after its tab was closed, without enabling input", async () => {
  const pending = deferred<TerminalHandle>();
  openTerminal.mockReturnValue(pending.promise);
  const view = render(TerminalInstance, { sessionId: "pending-a", kind, active: true, termTheme: {} });
  await waitFor(() => expect(openTerminal).toHaveBeenCalledOnce());
  expect(screen.getByRole("status")).toHaveTextContent("Connecting");
  view.unmount();
  const acquired = handle(); pending.resolve(acquired);
  await waitFor(() => expect(acquired.close).toHaveBeenCalledOnce());
  expect(acquired.write).not.toHaveBeenCalled();
  expect(terminal.input).toBeNull();
  expect(terminal.dispose).toHaveBeenCalledOnce();
});

it("keeps a closed tab ended after late acquisition failure", async () => {
  const pending = deferred<TerminalHandle>();
  openTerminal.mockReturnValue(pending.promise);
  const view = render(TerminalInstance, { sessionId: "pending-b", kind, active: true, termTheme: {} });
  await waitFor(() => expect(openTerminal).toHaveBeenCalledOnce());
  view.unmount(); pending.reject(new Error("engine unavailable"));
  await Promise.resolve(); await Promise.resolve();
  expect(terminal.input).toBeNull();
  expect(openTerminal).toHaveBeenCalledOnce();
  expect(terminal.writeln).not.toHaveBeenCalled();
});

it("normal Close releases the running handle without replaying input or opening another session", async () => {
  const acquired = handle(); openTerminal.mockResolvedValue(acquired);
  const view = render(TerminalInstance, { sessionId: "running-a", kind, active: true, termTheme: {} });
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Connected"));
  terminal.input?.("你好\r\u0003");
  expect(acquired.write).toHaveBeenCalledWith("你好\r\u0003");
  view.unmount();
  expect(acquired.close).toHaveBeenCalledOnce();
  expect(openTerminal).toHaveBeenCalledOnce();
});
