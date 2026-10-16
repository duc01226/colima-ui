import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import EngineConnectionSettings from "./EngineConnectionSettings.svelte";
import { engineConnectionState } from "../../lib/engineConnection.svelte";
const api = vi.hoisted(() => ({ get: vi.fn(), check: vi.fn(), save: vi.fn(), restart: vi.fn(), list: vi.fn(), notifications: vi.fn() }));
vi.mock("../../lib/api/system", () => ({ connectionApi: api }));
vi.mock("../../lib/api/transfer", () => ({ transferApi: { list: api.list } }));
vi.mock("../../store/capabilities.svelte", () => ({ capability: () => undefined, capabilityNotice: () => ({ text: "" }) }));
vi.mock("../../store/notifications.svelte", () => ({ openNotificationPanel: api.notifications }));
const active = { context: "desktop-linux", endpoint: "npipe:////./pipe/fixture", engine_id: "original", generation: 1, ready: true, error: null };
const snapshot = () => ({ active: { ...active }, pending: null as string | null, contexts: ["desktop-linux", "rancher-desktop"].map((name) => ({ name, supported: true, reason: null })), discovery_error: null });
beforeEach(() => {
  vi.resetAllMocks(); api.get.mockResolvedValue(snapshot()); api.check.mockImplementation(async (context: string) => ({ ...active, context })); api.save.mockResolvedValue(undefined); api.restart.mockResolvedValue(undefined); api.list.mockResolvedValue([]);
  // Keep renderer and component on one Svelte runtime; reset only owned state.
  Object.assign(engineConnectionState, { snapshot: null, loading: false, error: "", draft: null, checked: null, checkedContext: "", checking: false, saving: false, restarting: false });
  // DOM-only seam; native modal/focus/keyboard/NVDA qualification remains pending.
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function (this: HTMLDialogElement) { this.setAttribute("open", ""); });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (this: HTMLDialogElement) { this.removeAttribute("open"); });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
async function mountForm(onConnected?: () => void, expectedDraft = "desktop-linux") {
  render(EngineConnectionSettings, { id: "fixture-context", onConnected });
  await waitFor(() => expect(screen.getByRole("combobox")).toHaveValue(expectedDraft));
  expect(screen.getByText("Active engine: desktop-linux")).toBeInTheDocument();
}
// TC002/004/019/031: the actual form must not claim live connection or persistence.
it("keeps candidate check separate from the active engine and retains input after save refusal", async () => {
  const connected = vi.fn(); await mountForm(connected);
  expect(screen.getByRole("button", { name: "Continue to Containers" })).toBeEnabled();
  await fireEvent.change(screen.getByLabelText("Next launch"), { target: { value: "rancher-desktop" } });
  expect(screen.queryByRole("button", { name: "Continue to Containers" })).toBeNull();
  const save = screen.getByRole("button", { name: "Save for next launch" }); expect(save).toBeDisabled();
  await fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
  await waitFor(() => expect(save).toBeEnabled());
  expect(screen.getByText("Active engine: desktop-linux")).toBeInTheDocument();
  api.save.mockRejectedValueOnce(new Error("fixture persistence refused")); await fireEvent.click(save);
  await screen.findByRole("alert");
  expect(screen.getByRole("combobox")).toHaveValue("rancher-desktop");
  expect(screen.queryByText(/Saved rancher-desktop/)).toBeNull();
  expect(connected).not.toHaveBeenCalled(); expect(api.restart).not.toHaveBeenCalled();
});
it("blocks restart with active transfers and hands recovery to notifications", async () => {
  api.get.mockResolvedValueOnce({ ...snapshot(), pending: "rancher-desktop" });
  api.list.mockResolvedValueOnce([{ status: "running" }]); await mountForm(undefined, "rancher-desktop");
  await fireEvent.click(screen.getByRole("button", { name: "Restart ColimaUI" }));
  const modal = await screen.findByRole("dialog");
  await waitFor(() => expect(within(modal).getByRole("button", { name: "View transfers" })).toBeEnabled());
  expect(within(modal).queryByRole("button", { name: "Restart ColimaUI" })).toBeNull();
  await fireEvent.click(within(modal).getByRole("button", { name: "View transfers" }));
  expect(api.notifications).toHaveBeenCalledTimes(1); expect(api.restart).not.toHaveBeenCalled();
  expect(modal).not.toHaveAttribute("open");
});
it("cancel preserves pending choice and only explicit confirmation asks backend to restart", async () => {
  api.get.mockResolvedValueOnce({ ...snapshot(), pending: "rancher-desktop" }); await mountForm(undefined, "rancher-desktop");
  const open = screen.getByRole("button", { name: "Restart ColimaUI" }); await fireEvent.click(open);
  let modal = await screen.findByRole("dialog");
  await waitFor(() => expect(within(modal).getByRole("button", { name: "Restart ColimaUI" })).toBeEnabled());
  await fireEvent.click(within(modal).getByRole("button", { name: "Keep working" }));
  expect(api.restart).not.toHaveBeenCalled(); expect(open).toHaveFocus();
  await fireEvent.click(open); modal = await screen.findByRole("dialog");
  await waitFor(() => expect(within(modal).getByRole("button", { name: "Restart ColimaUI" })).toBeEnabled());
  await fireEvent.click(within(modal).getByRole("button", { name: "Restart ColimaUI" }));
  await waitFor(() => expect(api.restart).toHaveBeenCalledWith(false));
});
