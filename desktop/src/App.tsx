import { useState } from "react";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  Clock3,
  Home,
  Laptop,
} from "lucide-react";

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

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <button className="brand" type="button" onClick={() => setView("home")} aria-label="ReHome 首页">
          <span className="brand-mark" aria-hidden="true">
            R
          </span>
          <span>ReHome</span>
        </button>

        <nav className="navigation" aria-label="主导航">
          {views.map(({ id, label, accessibleLabel, icon: Icon }) => (
            <button
              className="nav-item"
              data-active={view === id}
              type="button"
              aria-label={accessibleLabel}
              aria-current={view === id ? "page" : undefined}
              onClick={() => setView(id)}
              key={id}
            >
              <Icon size={18} strokeWidth={1.8} aria-hidden="true" />
              <span>{label}</span>
            </button>
          ))}
        </nav>

        <div className="sidebar-meta">
          <Laptop size={16} aria-hidden="true" />
          <span>Desktop MVP</span>
        </div>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <span className="topbar-title">{viewTitles[view]}</span>
          <span className="status-chip">
            <span className="status-dot" aria-hidden="true" />
            本机
          </span>
        </header>

        {view === "home" ? (
          <div className="home-view">
            <section className="intro" aria-labelledby="home-title">
              <p className="eyebrow">CODEX WORKSPACE</p>
              <h1 id="home-title">迁移工作台</h1>
            </section>

            <section className="actions" aria-label="迁移操作">
              <button className="action-button action-send" type="button" onClick={() => setView("send")}>
                <ArrowUpFromLine size={23} strokeWidth={1.8} aria-hidden="true" />
                <span>发送</span>
              </button>
              <button className="action-button action-receive" type="button" onClick={() => setView("receive")}>
                <ArrowDownToLine size={23} strokeWidth={1.8} aria-hidden="true" />
                <span>接收</span>
              </button>
            </section>

            <section className="activity" aria-labelledby="activity-title">
              <div className="section-heading">
                <h2 id="activity-title">最近交接</h2>
                <Clock3 size={17} aria-hidden="true" />
              </div>
              <div className="empty-row">暂无交接记录</div>
            </section>
          </div>
        ) : (
          <section className="view-panel">
            <p className="eyebrow">REHOME</p>
            <h1>{viewTitles[view]}</h1>
            <div className="view-rule" aria-hidden="true" />
          </section>
        )}
      </main>
    </div>
  );
}
