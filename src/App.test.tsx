import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { CommandError } from "./api";
import { StoreProvider } from "./store";
import { app, catalogFixture, DEFAULT_SETTINGS, fakeApi, release } from "./test/fixtures";
import type { AppView } from "./types";

function setup(apps: AppView[] = catalogFixture()) {
  const f = fakeApi(apps);
  const user = userEvent.setup();
  render(
    <StoreProvider api={f.api}>
      <App />
    </StoreProvider>,
  );
  return { ...f, user };
}

async function goTo(user: ReturnType<typeof userEvent.setup>, label: string) {
  await user.click(await screen.findByRole("button", { name: new RegExp(`^${label}`) }));
}

describe("catalog", () => {
  it("lists all 12 Craft apps and ArtCraft separately", async () => {
    const { user } = setup();
    await goTo(user, "All Apps");
    for (const name of [
      "PhotoCraft",
      "VectorCraft",
      "FilmCraft",
      "LightCraft",
      "PDFCraft",
      "EffectCraft",
      "DesignCraft",
      "SoundCraft",
      "WordCraft",
      "GridCraft",
      "DeckCraft",
      "CADCraft",
    ]) {
      expect(await screen.findByRole("article", { name })).toBeInTheDocument();
    }
    expect(screen.getByRole("heading", { name: "Separate AI studio" })).toBeInTheDocument();
    const art = screen.getByRole("article", { name: "ArtCraft" });
    expect(within(art).getByText(/AI studio · separate product/)).toBeInTheDocument();
  });

  it("never offers Install for unavailable apps", async () => {
    const { user } = setup();
    await goTo(user, "All Apps");
    for (const name of ["SoundCraft", "ArtCraft"]) {
      const card = await screen.findByRole("article", { name });
      expect(within(card).queryByRole("button", { name: /^Install/ })).toBeNull();
      const btn = within(card).getByRole("button", { name: `${name} is unavailable` });
      expect(btn).toBeDisabled();
      expect(btn).toHaveAttribute("title");
    }
  });

  it("shows the unofficial disclaimer", async () => {
    setup();
    expect(
      (await screen.findAllByText(/Not affiliated with or endorsed by Storytold or ArtCraft/))
        .length,
    ).toBeGreaterThan(0);
  });
});

describe("install flow", () => {
  it("installs through the backend and shows real progress with cancel", async () => {
    const { api, user, emitProgress } = setup();
    await goTo(user, "All Apps");
    const card = await screen.findByRole("article", { name: "PhotoCraft" });
    await user.click(within(card).getByRole("button", { name: "Install PhotoCraft 1.0.0" }));
    const dialog = await screen.findByRole("dialog", { name: "Install PhotoCraft" });
    await user.click(within(dialog).getByRole("button", { name: /^Install$/ }));
    expect(api.installApp).toHaveBeenCalledWith("photocraft", undefined, "C:\\Apps", true);

    act(() =>
      emitProgress({
        opId: "op9",
        appId: "photocraft",
        kind: "install",
        phase: "downloading",
        done: 50,
        total: 200,
        message: null,
      }),
    );
    const bar = await within(card).findByRole("progressbar");
    expect(bar).toHaveAttribute("aria-valuenow", "25");
    await user.click(within(card).getByRole("button", { name: /Cancel/ }));
    expect(api.cancelOperation).toHaveBeenCalledWith("op9");

    act(() =>
      emitProgress({
        opId: "op9",
        appId: "photocraft",
        kind: "install",
        phase: "activating",
        done: 0,
        total: 1,
        message: null,
      }),
    );
    expect(within(card).getByRole("button", { name: /Cancel/ })).toBeDisabled();
  });

  it("reports backend errors honestly", async () => {
    const { api, user } = setup();
    api.installApp.mockRejectedValueOnce(
      new CommandError({ kind: "integrity", message: "Integrity check failed: mismatch" }),
    );
    await goTo(user, "All Apps");
    const card = await screen.findByRole("article", { name: "VectorCraft" });
    await user.click(within(card).getByRole("button", { name: /^Install VectorCraft/ }));
    const dialog = await screen.findByRole("dialog", { name: "Install VectorCraft" });
    await user.click(within(dialog).getByRole("button", { name: /^Install$/ }));
    // Shown inline on the card (with Retry) and as a toast.
    const alerts = await screen.findAllByRole("alert");
    expect(alerts.some((a) => /Integrity check failed: mismatch/.test(a.textContent ?? ""))).toBe(
      true,
    );
  });
});

describe("installed apps", () => {
  const installed = (patch: Partial<AppView> = {}) =>
    app("photocraft", "PhotoCraft", {
      status: "updateAvailable",
      updateAvailable: true,
      latest: release("0.3.0"),
      installed: {
        version: "0.2.0",
        tag: "v0.2.0",
        path: "C:\\Apps\\photocraft\\0.2.0_x",
        executable: "photocraft.exe",
        installedAt: 1_790_000_000,
        sha256: "ab".repeat(32),
        verification: "github-digest+sha256sums",
        previousVersion: null,
      },
      ...patch,
    });

  it("disables Update while the app is running and explains why", async () => {
    const { user } = setup([installed({ running: true })]);
    await goTo(user, "Updates");
    const btn = await screen.findByRole("button", { name: "Update PhotoCraft to 0.3.0" });
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute("title", "Close PhotoCraft to update it");
  });

  it("opens the app via the backend", async () => {
    const { api, user } = setup([installed()]);
    // Home shows both a launcher tile and the update card; each opens the app.
    const buttons = await screen.findAllByRole("button", { name: "Open PhotoCraft" });
    expect(buttons.length).toBe(2);
    await user.click(buttons[0]!);
    expect(api.launchApp).toHaveBeenCalledWith("photocraft");
  });

  it("requires confirmation before uninstalling", async () => {
    const { api, user } = setup([installed({ status: "installed", updateAvailable: false })]);
    await goTo(user, "Installed");
    await user.click(await screen.findByRole("button", { name: "PhotoCraft details" }));
    await user.click(screen.getByRole("button", { name: /Uninstall PhotoCraft/ }));
    expect(api.uninstallApp).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog", { name: "Uninstall PhotoCraft?" });
    await user.click(within(dialog).getByRole("button", { name: "Uninstall" }));
    expect(api.uninstallApp).toHaveBeenCalledWith("photocraft");
  });

  it("renders release notes as text, not HTML", async () => {
    const { user } = setup([installed()]);
    await goTo(user, "Updates");
    await user.click(await screen.findByRole("button", { name: "PhotoCraft details" }));
    expect(screen.getByText(/Notes <script>alert\(1\)<\/script>/)).toBeInTheDocument();
    expect(document.querySelector("script")).toBeNull();
  });

  it("summarizes Update All results", async () => {
    const { api, user } = setup([installed()]);
    api.updateAll.mockResolvedValueOnce({
      updated: [],
      skipped: [
        {
          appId: "photocraft",
          name: "PhotoCraft",
          from: "0.2.0",
          to: "0.3.0",
          reason: "PhotoCraft is running.",
        },
      ],
      failed: [],
    });
    await goTo(user, "Updates");
    await user.click(screen.getByRole("button", { name: /Update all/ }));
    const dialog = await screen.findByRole("dialog", { name: "Update All finished" });
    expect(within(dialog).getByText("PhotoCraft: PhotoCraft is running.")).toBeInTheDocument();
  });
});

describe("phase 2 UX", () => {
  it("offers Retry after a failed install and retries the same action", async () => {
    const { api, user } = setup();
    api.installApp.mockRejectedValueOnce(
      new CommandError({ kind: "network", message: "Network error: connection reset" }),
    );
    await goTo(user, "All Apps");
    const card = await screen.findByRole("article", { name: "FilmCraft" });
    await user.click(within(card).getByRole("button", { name: /^Install FilmCraft/ }));
    const dialog = await screen.findByRole("dialog", { name: "Install FilmCraft" });
    await user.click(within(dialog).getByRole("button", { name: /^Install$/ }));
    const failure = await within(card).findByText(/Install failed/);
    expect(failure.closest("[role=alert]")).toHaveTextContent("Check your connection");
    await user.click(within(card).getByRole("button", { name: /Retry/ }));
    expect(api.installApp).toHaveBeenCalledTimes(2);
    expect(api.installApp).toHaveBeenLastCalledWith("filmcraft", undefined);
  });

  it("opens the app details when a notification is clicked", async () => {
    const { emitNavigate } = setup();
    await screen.findAllByRole("article");
    act(() => emitNavigate({ view: "app", appId: "vectorcraft" }));
    expect(await screen.findByRole("dialog", { name: "VectorCraft" })).toBeInTheDocument();
  });

  it("asks before quitting while installs run", async () => {
    const { api, user, emitConfirmExit } = setup();
    await screen.findAllByRole("article");
    act(() => emitConfirmExit([{ opId: "o1", appId: "photocraft" }]));
    const dialog = await screen.findByRole("dialog", { name: "Quit while installing?" });
    expect(dialog).toHaveTextContent("PhotoCraft");
    await user.click(within(dialog).getByRole("button", { name: "Keep working" }));
    expect(api.exitApp).not.toHaveBeenCalled();
    act(() => emitConfirmExit([{ opId: "o1", appId: "photocraft" }]));
    await user.click(await screen.findByRole("button", { name: "Cancel and quit" }));
    expect(api.exitApp).toHaveBeenCalled();
  });

  it("changes update mode and install location through the backend only", async () => {
    const { api, user } = setup();
    await goTo(user, "Settings");
    await user.click(await screen.findByRole("radio", { name: /Automatic/ }));
    expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ updateMode: "automatic" }),
    );
    await user.click(screen.getByRole("checkbox", { name: /notification area/ }));
    expect(api.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ minimizeToTray: false }),
    );
    await user.click(screen.getByRole("button", { name: /Change…/ }));
    expect(api.chooseInstallRoot).toHaveBeenCalledWith();
    expect(await screen.findByText(/This build has no update-signing key/)).toBeInTheDocument();
  });

  it("disables background options in Manual mode", async () => {
    const f = fakeApi(catalogFixture());
    f.api.getSettings.mockResolvedValue({ ...DEFAULT_SETTINGS, updateMode: "manual" });
    const user = userEvent.setup();
    render(
      <StoreProvider api={f.api}>
        <App />
      </StoreProvider>,
    );
    await goTo(user, "Settings");
    expect(await screen.findByRole("combobox")).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: /Windows notifications/ })).toBeDisabled();
  });

  it("explains offline mode", async () => {
    const apps = catalogFixture().map((a) => ({
      ...a,
      check: { ...a.check, source: "staleCache" as const, error: "GitHub could not be reached." },
    }));
    setup(apps);
    expect(await screen.findByText(/GitHub can't be reached right now/)).toBeInTheDocument();
    expect(screen.getByText(/Installed apps still open/)).toBeInTheDocument();
  });
});
