import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { tick } from "svelte";
import { globalToast } from "../../lib/globalToast";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
const native = vi.hoisted(() => ({ save: vi.fn(), dirname: vi.fn(), basename: vi.fn(), saveImages: vi.fn() }));
vi.mock("../../lib/env", () => ({ isRunningInTauri: () => true }));
vi.mock("../../store/capabilities.svelte", () => ({ isDockerOnlyHost: () => true }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: native.save }));
vi.mock("@tauri-apps/api/path", () => ({ dirname: native.dirname, basename: native.basename }));
vi.mock("../../lib/api/transfer", () => ({ transferApi: { saveImages: native.saveImages } }));
vi.mock("../../lib/globalToast", () => ({ globalToast: vi.fn() }));
vi.mock("../../store/notifications.svelte", () => ({ startJob: vi.fn() }));
import TransferDialog from "./TransferDialog.svelte";
beforeEach(() => { vi.resetAllMocks(); native.saveImages.mockResolvedValue({ jobId: "owned", totalEstimate: 1 }); });
afterEach(cleanup);
// TC010/019: actual component field-to-API handoff, native path API substituted.
// Native picker/IPC/filesystem behavior still requires Windows qualification.
it.each([
  [String.raw`C:\Users\名字\My Exports\images.tar`, String.raw`C:\Users\名字\My Exports`, "images.tar", "images.tar"],
  [String.raw`D:\images`, "D:\\", "images", "images.tar"],
  [String.raw`\\server\share\Images\archive.tar`, String.raw`\\server\share\Images`, "archive.tar", "archive.tar"],
  ["/images.tar", "/", "images.tar", "images.tar"],
])("preserves native parent and file for %s", async (chosen, folder, name, finalName) => {
  native.save.mockResolvedValueOnce(chosen); native.dirname.mockResolvedValueOnce(folder); native.basename.mockResolvedValueOnce(name);
  render(TransferDialog, { mode: "export", images: ["fixture:owned"], onClose: () => {} });
  await fireEvent.click(screen.getByRole("button", { name: /Browse/ }));
  const start = screen.getByRole("button", { name: /Start in background/i });
  await waitFor(() => expect(start).toBeEnabled());
  expect(native.dirname).toHaveBeenCalledWith(chosen); expect(native.basename).toHaveBeenCalledWith(chosen);
  await fireEvent.click(start);
  await waitFor(() => expect(native.saveImages).toHaveBeenCalledWith(["fixture:owned"], folder, finalName, false));
});
it.each(["cancel", "path-error"])("retains typed fields on native picker %s", async (mode) => {
  native.save.mockResolvedValueOnce(mode === "cancel" ? null : "C:\\new\\chosen.tar");
  native.dirname.mockRejectedValueOnce(new Error("native path refused")); native.basename.mockResolvedValueOnce("chosen.tar");
  render(TransferDialog, { mode: "export", images: ["fixture:owned"], onClose: () => {} });
  await fireEvent.input(screen.getByLabelText(/Folder on this machine/), { target: { value: "C:\\kept" } });
  await fireEvent.click(screen.getByRole("button", { name: /Browse/ }));
  await waitFor(() => expect(native.save).toHaveBeenCalledTimes(1));
  await native.save.mock.results[0].value;
  await tick();
  if (mode === "path-error") await waitFor(() => expect(globalToast).toHaveBeenCalledWith("error", expect.stringContaining("native path refused")));
  await fireEvent.click(screen.getByRole("button", { name: /Start in background/i }));
  await waitFor(() => expect(native.saveImages).toHaveBeenCalledWith(["fixture:owned"], "C:\\kept", "images.tar", false));
  if (mode === "cancel") expect(native.dirname).not.toHaveBeenCalled();
});
