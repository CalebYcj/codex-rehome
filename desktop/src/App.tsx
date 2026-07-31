import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  CheckCircle2,
  Clock3,
  Home,
  Laptop,
  LoaderCircle,
  TriangleAlert,
} from "lucide-react";

import HomePage from "./features/home/HomePage";
import HistoryPage from "./features/history/HistoryPage";
import ReceivePage from "./features/receive/ReceivePage";
import SendPage from "./features/send/SendPage";
import UpdateControl from "./features/update/UpdateControl";
import { discoverCodex } from "./lib/api";
import { errorMessage, type CodexInventory } from "./lib/types";
import "./App.css";

export type View = "home" | "send" | "receive" | "history";

const views: Array<{
  id: View;
  label: string;
  accessibleLabel: string;
  icon: typeof Home;
}> = [
  { id: "home", label: "首页", accessibleLabel: "前往首页", icon: Home },
  { id: "send", label: "发送", accessibleLabel: "前往发送", icon: ArrowUpFromLine },
  { id: "receive", label: "接收", accessibleLabel: "前往接收", icon: ArrowDownToLine },
  { id: "history", label: "历史", accessibleLabel: "前往历史", icon: Clock3 },
];

const viewTitles: Record<View, string> = {
  home: "迁移工作台",
  send: "发送交接",
  receive: "接收交接",
  history: "历史记录",
};

export default function App() {
  const [view, setView] = useState<View>("home");
  const [inventory, setInventory] = useState<CodexInventory | null>(null);
  const [loading, setLoading] = useState(true);
  const [discoveryError, setDiscoveryError] = useState<string | null>(null);
  const [activeOperations, setActiveOperations] = useState(0);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const previousViewRef = useRef(view);

  useEffect(() => {
    let active = true;
    void discoverCodex()
      .then((detected) => {
        if (active) setInventory(detected);
      })
      .catch((caught) => {
        if (active) setDiscoveryError(errorMessage(caught));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (previousViewRef.current !== view) {
      headingRef.current?.focus();
      previousViewRef.current = view;
    }
  }, [view]);

  function navigate(next: View) {
    setView(next);
  }

  const operationStarted = useCallback(() => {
    setActiveOperations((current) => current + 1);
  }, []);

  const operationFinished = useCallback(() => {
    setActiveOperations((current) => Math.max(0, current - 1));
  }, []);

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <button className="brand" type="button" onClick={() => navigate("home")} aria-label="ReHome 首页">
          <span className="brand-mark" aria-hidden="true">R</span>
          <span className="brand-copy"><strong>ReHome</strong><small>Desktop</small></span>
        </button>

        <nav className="navigation" aria-label="主导航">
          {views.map(({ id, label, accessibleLabel, icon: Icon }) => (
            <button
              className="nav-item"
              data-active={view === id}
              type="button"
              aria-label={accessibleLabel}
              title={label}
              aria-current={view === id ? "page" : undefined}
              onClick={() => navigate(id)}
              key={id}
            >
              <Icon aria-hidden="true" />
              <span>{label}</span>
            </button>
          ))}
        </nav>

        <UpdateControl migrationBusy={activeOperations > 0} />
        <div className="sidebar-meta">
          <Laptop aria-hidden="true" />
          <span>离线本机迁移</span>
        </div>
      </aside>

      <main className="workspace" data-view={view}>
        <header className="topbar">
          <span className="topbar-title">{viewTitles[view]}</span>
          {loading ? (
            <span className="machine-status"><LoaderCircle className="spin" aria-hidden="true" />正在检测</span>
          ) : discoveryError ? (
            <span className="machine-status machine-error"><TriangleAlert aria-hidden="true" />未检测到 Codex</span>
          ) : (
            <span className="machine-status"><CheckCircle2 aria-hidden="true" />本机已就绪</span>
          )}
        </header>

        {view === "home" && <HomePage headingRef={headingRef} inventory={inventory} loading={loading} error={discoveryError} onNavigate={navigate} />}
        {view === "send" && <SendPage headingRef={headingRef} inventory={inventory} onOperationStart={operationStarted} onOperationEnd={operationFinished} />}
        {view === "receive" && <ReceivePage headingRef={headingRef} inventory={inventory} onOperationStart={operationStarted} onOperationEnd={operationFinished} />}
        {view === "history" && <HistoryPage headingRef={headingRef} onOperationStart={operationStarted} onOperationEnd={operationFinished} />}
      </main>
    </div>
  );
}
