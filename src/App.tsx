import { useState } from "react";
import { AppDrawer } from "./components/AppDrawer";
import { InstallDialog } from "./components/InstallDialog";
import {
  ExitDialog,
  OfflineBanner,
  Sidebar,
  Toasts,
  TopBar,
  UpdateAllSummaryDialog,
} from "./components/Chrome";
import { useStore } from "./store";
import { AppsView, HomeView, SettingsView } from "./views/Views";

export function App() {
  const s = useStore();
  const { view, setView, detailsId, setDetailsId } = s;
  const [query, setQuery] = useState("");
  const details = detailsId ? s.apps.find((a) => a.id === detailsId) : undefined;

  return (
    <div className="shell">
      <Sidebar view={view} onNavigate={setView} />
      <main className="main" id="main">
        {view !== "settings" && <TopBar query={query} onQuery={setQuery} />}
        {view !== "settings" && <OfflineBanner />}
        {view === "home" && <HomeView query={query} onDetails={setDetailsId} />}
        {(view === "all" || view === "installed" || view === "updates") && (
          <AppsView filter={view} query={query} onDetails={setDetailsId} />
        )}
        {view === "settings" && <SettingsView />}
      </main>
      {/* keyed so per-app drawer state (loaded versions, dialogs) resets when switching apps */}
      {details && <AppDrawer key={details.id} app={details} onClose={() => setDetailsId(null)} />}
      <UpdateAllSummaryDialog />
      <InstallDialog />
      <ExitDialog />
      <Toasts />
    </div>
  );
}
