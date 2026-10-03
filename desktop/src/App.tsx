import { useCallback, useEffect, useRef, useState } from "react";
import {
  Clock3,
  HelpCircle,
  Languages,
  MoreHorizontal,
  Settings,
  X,
} from "lucide-react";
import HomePage from "./features/home/HomePage";
import HistoryPage from "./features/history/HistoryPage";
import ReceivePage from "./features/receive/ReceivePage";
import SendPage from "./features/send/SendPage";
import HelpPage from "./features/help/HelpPage";
import UpdateControl from "./features/update/UpdateControl";
import StepBar from "./components/StepBar";
import { discoverCodex } from "./lib/api";
import { I18nProvider, useI18n } from "./lib/i18n";
import { errorMessage, type CodexInventory } from "./lib/types";
import "./App.css";

export type View =
  | "home"
  | "send"
  | "receive"
  | "history"
  | "settings"
  | "help";
export default function App() {
  return (
    <I18nProvider>
      <AppContent />
    </I18nProvider>
  );
}
function AppContent() {
  const { locale, setLocale, t } = useI18n();
  const [view, setView] = useState<View>("home");
  const [returnView, setReturnView] = useState<View>("home");
  const [visited, setVisited] = useState<Set<View>>(new Set(["home"]));
  const [menuOpen, setMenuOpen] = useState(false);
  const [inventory, setInventory] = useState<CodexInventory | null>(null);
  const [loading, setLoading] = useState(true);
  const [discoveryError, setDiscoveryError] = useState<string | null>(null);
  const [activeOperations, setActiveOperations] = useState(0);
  const [updateInstalling, setUpdateInstalling] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuButtonRef = useRef<HTMLButtonElement>(null);
  const homeRef = useRef<HTMLHeadingElement>(null),
    sendRef = useRef<HTMLHeadingElement>(null),
    receiveRef = useRef<HTMLHeadingElement>(null),
    historyRef = useRef<HTMLHeadingElement>(null),
    helpRef = useRef<HTMLHeadingElement>(null),
    settingsRef = useRef<HTMLHeadingElement>(null);
  const locked = activeOperations > 0 || updateInstalling;
  useEffect(() => {
    let active = true;
    void discoverCodex()
      .then((result) => {
        if (active) setInventory(result);
      })
      .catch((error) => {
        if (active) setDiscoveryError(errorMessage(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);
  useEffect(() => {
    ({
      home: homeRef,
      send: sendRef,
      receive: receiveRef,
      history: historyRef,
      help: helpRef,
      settings: settingsRef,
    })[view].current?.focus();
  }, [view]);
  useEffect(() => {
    if (!menuOpen) return;
    menuRef.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    function outside(event: PointerEvent) {
      if (
        !menuRef.current?.contains(event.target as Node) &&
        !menuButtonRef.current?.contains(event.target as Node)
      )
        setMenuOpen(false);
    }
    function keyboard(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setMenuOpen(false);
        menuButtonRef.current?.focus();
      }
      if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
        event.preventDefault();
        const items = [
          ...(menuRef.current?.querySelectorAll<HTMLElement>(
            '[role="menuitem"]',
          ) ?? []),
        ];
        const i = items.indexOf(document.activeElement as HTMLElement);
        const next =
          event.key === "Home"
            ? 0
            : event.key === "End"
              ? items.length - 1
              : (i + (event.key === "ArrowDown" ? 1 : -1) + items.length) %
                items.length;
        items[next]?.focus();
      }
      if (event.key === "Tab") setMenuOpen(false);
    }
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", keyboard);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", keyboard);
    };
  }, [menuOpen]);
  function navigate(next: View) {
    if (locked) return;
    if (
      ["help", "settings", "history"].includes(next) &&
      ["home", "send", "receive"].includes(view)
    )
      setReturnView(view);
    setMenuOpen(false);
    setVisited((current) => new Set(current).add(next));
    setView(next);
  }
  const operationStarted = useCallback(
    () => setActiveOperations((value) => value + 1),
    [],
  );
  const operationFinished = useCallback(
    () => setActiveOperations((value) => Math.max(0, value - 1)),
    [],
  );
  const workflowProps = {
    onOperationStart: operationStarted,
    onOperationEnd: operationFinished,
  };
  return (
    <div className="app-shell">
      <header className="app-header">
        <button
          className="brand"
          type="button"
          onClick={() => navigate("home")}
          disabled={locked}
          aria-label={t("ReHome 首页")}
        >
          <svg className="brand-mark" viewBox="0 0 32 32" aria-hidden="true">
            <path
              d="M13 24H5V5h15v8M19 8h8v19H12v-8M12 16h9"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.1"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
          <strong>ReHome</strong>
        </button>
        <div className="menu-anchor">
          <button
            ref={menuButtonRef}
            className="icon-button"
            type="button"
            aria-label={t("更多选项")}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            aria-controls="app-menu"
            disabled={locked}
            onClick={() => setMenuOpen(!menuOpen)}
          >
            {menuOpen ? (
              <X aria-hidden="true" />
            ) : (
              <MoreHorizontal aria-hidden="true" />
            )}
          </button>
          {menuOpen && (
            <div
              className="app-menu"
              id="app-menu"
              role="menu"
              ref={menuRef}
              aria-label={t("更多选项")}
            >
              <button role="menuitem" onClick={() => navigate("history")}>
                <Clock3 aria-hidden="true" />
                {t("迁移记录")}
              </button>
              <button role="menuitem" onClick={() => navigate("help")}>
                <HelpCircle aria-hidden="true" />
                {t("帮助")}
              </button>
              <button role="menuitem" onClick={() => navigate("settings")}>
                <Settings aria-hidden="true" />
                {t("设置与关于")}
              </button>
              <button
                role="menuitem"
                onClick={() => {
                  setLocale(locale === "en" ? "zh-CN" : "en");
                  setMenuOpen(false);
                }}
              >
                <Languages aria-hidden="true" />
                {locale === "en" ? "中文" : "English"}
              </button>
            </div>
          )}
        </div>
      </header>
      <main
        className="workspace"
        data-view={view}
        inert={updateInstalling ? true : undefined}
        aria-busy={updateInstalling}
      >
        {view === "home" && (
          <HomePage
            headingRef={homeRef}
            inventory={inventory}
            loading={loading}
            error={discoveryError}
            onNavigate={navigate}
          />
        )}
        {visited.has("send") && (
          <div hidden={view !== "send"}>
            <SendPage
              headingRef={sendRef}
              inventory={inventory}
              {...workflowProps}
              onHome={() => navigate("home")}
            />
          </div>
        )}
        {visited.has("receive") && (
          <div hidden={view !== "receive"}>
            <ReceivePage
              headingRef={receiveRef}
              inventory={inventory}
              {...workflowProps}
              onHome={() => navigate("home")}
              onHistory={() => navigate("history")}
            />
          </div>
        )}
        {view === "history" && (
          <>
            <div className="aux-back">
              <StepBar
                label={t("迁移记录")}
                onBack={() => navigate(returnView)}
                busy={locked}
              />
            </div>
            <HistoryPage headingRef={historyRef} {...workflowProps} />
          </>
        )}
        {view === "help" && (
          <>
            <div className="aux-back">
              <StepBar label={t("帮助")} onBack={() => navigate(returnView)} />
            </div>
            <HelpPage headingRef={helpRef} />
          </>
        )}
        <div hidden={view !== "settings"}>
          <div className="aux-back">
            <StepBar
              label={t("设置")}
              onBack={() => navigate(returnView)}
              busy={locked}
            />
          </div>
          <div className="page settings-page">
            <header className="page-header">
              <h1 tabIndex={-1} ref={settingsRef}>
                {t("设置与关于")}
              </h1>
              <p className="page-description">
                {t("保持简单，只设置必要的事。")}
              </p>
            </header>
            <section className="workflow-section">
              <div className="form-row">
                <span>{t("界面语言")}</span>
                <button
                  className="secondary-button"
                  disabled={locked}
                  aria-label={
                    locale === "en" ? "切换为中文" : "Switch to English"
                  }
                  onClick={() => setLocale(locale === "en" ? "zh-CN" : "en")}
                >
                  {locale === "en" ? "中文" : "English"}
                </button>
              </div>
            </section>
            <section className="workflow-section">
              <h2>{t("版本与更新")}</h2>
              <UpdateControl
                migrationBusy={activeOperations > 0}
                onInstallingChange={setUpdateInstalling}
              />
            </section>
            <section className="workflow-section">
              <div className="form-row">
                <span>{t("本机帮助")}</span>
                <button
                  className="icon-text-button"
                  onClick={() => navigate("help")}
                >
                  {t("查看帮助")} →
                </button>
              </div>
              <p className="privacy-note">
                {t("迁移内容留在本机。ReHome 不会上传项目、对话或登录信息。")}
              </p>
              <p className="privacy-note">ReHome Desktop · {t("免费且开源")}</p>
            </section>
          </div>
        </div>
      </main>
    </div>
  );
}
