import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Archive,
  ArrowDownWideNarrow,
  CheckCheck,
  ChevronDown,
  ChevronRight,
  Download,
  Folder,
  FolderOpen,
  Info,
  Loader2,
  Menu,
  MessageSquare,
  Moon,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Settings2,
  Star,
  Sun,
  X,
} from "lucide-react";
import type {
  BatchRequest,
  Bootstrap,
  SessionIndex,
  SessionSummary,
  TerminalPreference,
} from "../shared/types";
import { api, relativeTime } from "./api";
import { Detail } from "./Detail";
import { Modal } from "./Modal";

type View = "all" | "starred" | "archived";
type Dialog =
  | { type: "rename"; keys: string[] }
  | { type: "archive"; keys: string[]; restore: boolean }
  | { type: "launch"; session?: SessionSummary }
  | { type: "export"; keys: string[] }
  | { type: "settings" | "help" };
const titles: Record<View, string> = {
  all: "全部会话",
  starred: "我的收藏",
  archived: "已归档",
};
function useTheme() {
  const [dark, setDark] = useState(() => {
    try {
      return localStorage.getItem("pi-theme") === "dark";
    } catch {
      return false;
    }
  });
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    try {
      localStorage.setItem("pi-theme", dark ? "dark" : "light");
    } catch {
      /* Storage may be disabled. */
    }
  }, [dark]);
  return [dark, setDark] as const;
}
export default function App() {
  const [config, setConfig] = useState<Bootstrap>();
  const [index, setIndex] = useState<SessionIndex>({
    sessions: [],
    warnings: [],
    scannedAt: "",
  });
  const [loading, setLoading] = useState(true);
  const [connected, setConnected] = useState(false);
  const [fatalError, setFatalError] = useState("");
  const [revision, setRevision] = useState(0);
  const [view, setView] = useState<View>("all");
  const [project, setProject] = useState("");
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState("updated");
  const [selectedKey, setSelectedKey] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [pageSize, setPageSize] = useState(100);
  const [dialog, setDialog] = useState<Dialog>();
  const [name, setName] = useState("");
  const [projectKey, setProjectKey] = useState("");
  const [newCwd, setNewCwd] = useState("");
  const [fork, setFork] = useState(false);
  const [busy, setBusy] = useState(false);
  const [dialogError, setDialogError] = useState("");
  const [toast, setToast] = useState<{ text: string; error: boolean }>();
  const [dark, setDark] = useTheme();
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [mobileDetail, setMobileDetail] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );
  const loadSequence = useRef(0);
  const notify = useCallback((text: string, error = false) => {
    clearTimeout(toastTimer.current);
    setToast({ text, error });
    toastTimer.current = setTimeout(() => setToast(undefined), 5500);
  }, []);
  const loadIndex = useCallback(async (refresh = false) => {
    const sequence = ++loadSequence.current;
    try {
      const data = await (refresh ? api.refresh() : api.index());
      if (sequence === loadSequence.current) {
        setIndex(data);
        setFatalError("");
        setConnected(true);
        setRevision((r) => r + 1);
      }
    } catch (error) {
      if (sequence === loadSequence.current) {
        setFatalError((error as Error).message);
        setConnected(false);
      }
      throw error;
    } finally {
      if (sequence === loadSequence.current) setLoading(false);
    }
  }, []);
  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void (async () => {
      const data = await api.bootstrap();
      if (!active) return;
      setConfig(data);
      const stop = await api.subscribe(
        () => {
          if (active)
            void loadIndex().catch((error) => notify(error.message, true));
        },
        (error) => {
          if (active) {
            setConnected(false);
            notify(error, true);
          }
        },
      );
      if (!active) {
        stop();
        return;
      }
      unsubscribe = stop;
      await loadIndex();
    })().catch((error) => {
      if (active) {
        setFatalError(error.message);
        setLoading(false);
        setConnected(false);
      }
    });
    return () => {
      active = false;
      unsubscribe?.();
      clearTimeout(toastTimer.current);
    };
  }, [loadIndex, notify]);
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (!dialog) searchRef.current?.focus();
      }
      if (
        event.key === "?" &&
        !dialog &&
        !["INPUT", "TEXTAREA", "SELECT"].includes(
          (event.target as HTMLElement).tagName,
        )
      )
        setDialog({ type: "help" });
    };
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, [dialog]);
  const sessions = index.sessions;
  const projects = useMemo(() => {
    const groups = new Map<
      string,
      { name: string; count: number; key: string }
    >();
    sessions
      .filter((s) => !s.archived)
      .forEach((s) => {
        const item = groups.get(s.cwd);
        if (item) item.count++;
        else groups.set(s.cwd, { name: s.project, count: 1, key: s.key });
      });
    return [...groups.entries()].sort((a, b) => b[1].count - a[1].count);
  }, [sessions]);
  const filtered = useMemo(() => {
    const query = search.toLocaleLowerCase().trim();
    return sessions
      .filter(
        (s) =>
          (view === "archived" ? s.archived : !s.archived) &&
          (view !== "starred" || s.starred) &&
          (!project || s.cwd === project) &&
          (!query ||
            [s.name, s.preview, s.cwd, s.model, s.id]
              .join(" ")
              .toLocaleLowerCase()
              .includes(query)),
      )
      .sort((a, b) =>
        sort === "name"
          ? a.name.localeCompare(b.name, "zh-CN")
          : sort === "messages"
            ? b.messageCount - a.messageCount
            : b.updatedAt.localeCompare(a.updatedAt),
      );
  }, [sessions, view, project, search, sort]);
  useEffect(() => {
    if (!sessions.some((s) => s.key === selectedKey))
      setSelectedKey(filtered[0]?.key || "");
  }, [sessions, filtered, selectedKey]);
  useEffect(() => {
    setSelected(new Set());
    setPageSize(100);
  }, [view, project, search]);
  useEffect(() => {
    const keys = new Set(sessions.map((s) => s.key));
    setSelected(
      (previous) => new Set([...previous].filter((key) => keys.has(key))),
    );
  }, [sessions]);
  const selectedTerminal =
    config?.terminalPreference === "auto"
      ? config.terminalOptions.find((terminal) => terminal.available)?.id || ""
      : config?.terminalPreference || "";
  const current = sessions.find((s) => s.key === selectedKey);
  const shown = filtered.slice(0, pageSize);
  const allChecked =
    shown.length > 0 && shown.every((s) => selected.has(s.key));
  const activeSessions = sessions.filter((s) => !s.archived);
  const switchView = (next: View, cwd = "") => {
    setView(next);
    setProject(cwd);
    setSelectedKey("");
    setSidebarOpen(false);
    setMobileDetail(false);
  };
  const toggleSelection = (key: string) =>
    setSelected((previous) => {
      const next = new Set(previous);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  const openDialog = (next: Dialog) => {
    setDialogError("");
    setDialog(next);
    setFork(false);
    setNewCwd("");
    if (next.type === "rename")
      setName(
        next.keys.length === 1
          ? sessions.find((s) => s.key === next.keys[0])?.name || ""
          : "{name}",
      );
    if (next.type === "launch")
      setProjectKey(projects.find(([cwd]) => cwd === project)?.[1].key || "");
  };
  const closeDialog = () => {
    if (!busy) setDialog(undefined);
  };
  const mutate = async (data: BatchRequest) => {
    await api.batch(data);
    await loadIndex();
  };
  const quickStar = (session: SessionSummary) => {
    void mutate({
      keys: [session.key],
      action: session.starred ? "unstar" : "star",
    }).catch((error) => notify(error.message, true));
  };
  const confirmDialog = async () => {
    if (!dialog || busy) return;
    setBusy(true);
    setDialogError("");
    try {
      if (dialog.type === "rename") {
        await mutate({ action: "rename", keys: dialog.keys, name });
        notify(`已重命名 ${dialog.keys.length} 个会话`);
      }
      if (dialog.type === "archive") {
        await mutate({
          action: dialog.restore ? "restore" : "archive",
          keys: dialog.keys,
        });
        setSelected(new Set());
        notify(
          dialog.restore ? "会话已恢复到列表" : "会话已归档，原文件保持不变",
        );
      }
      if (dialog.type === "export") {
        const result = await api.export(dialog.keys);
        if (result.saved) notify("会话已保存到所选目录");
      }
      if (dialog.type === "launch") {
        const result = await api.launch(
          dialog.session
            ? { key: dialog.session.key, fork }
            : {
                projectKey: newCwd ? undefined : projectKey || undefined,
                cwd: newCwd || undefined,
              },
        );
        notify(result.message);
      }
      setDialog(undefined);
    } catch (error) {
      setDialogError((error as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const refresh = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await loadIndex(true);
      notify("会话列表已更新");
    } catch (error) {
      notify((error as Error).message, true);
    } finally {
      setBusy(false);
    }
  };
  const changeRoot = async (reset = false) => {
    setBusy(true);
    try {
      const path = reset
        ? null
        : await api.chooseDirectory(config?.sessionRoot);
      if (!reset && !path) return;
      const next = await api.setRoot(path);
      setConfig(next);
      setProject("");
      setView("all");
      setSelectedKey("");
      setSelected(new Set());
      await loadIndex();
      notify("会话目录已更新");
    } catch (error) {
      notify((error as Error).message, true);
    } finally {
      setBusy(false);
    }
  };
  const changeTerminal = async (preference: TerminalPreference) => {
    if (busy) return;
    setBusy(true);
    try {
      setConfig(await api.setTerminal(preference));
      notify("默认终端已保存");
    } catch (error) {
      notify((error as Error).message, true);
    } finally {
      setBusy(false);
    }
  };
  const chooseWorkspace = async () => {
    try {
      const path = await api.chooseDirectory(config?.defaultCwd);
      if (path) setNewCwd(path);
    } catch (error) {
      setDialogError((error as Error).message);
    }
  };
  const batchStar = async () => {
    setBusy(true);
    try {
      await mutate({
        action: view === "starred" ? "unstar" : "star",
        keys: [...selected],
      });
      notify(view === "starred" ? "已取消收藏" : "已收藏所选会话");
      setSelected(new Set());
    } catch (error) {
      notify((error as Error).message, true);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className={`app-shell ${mobileDetail ? "show-detail" : ""}`}>
      {sidebarOpen && (
        <button
          className="sidebar-scrim"
          aria-label="关闭导航"
          onClick={() => setSidebarOpen(false)}
        />
      )}
      <aside className={`sidebar ${sidebarOpen ? "sidebar-open" : ""}`}>
        <div className="brand">Pi Sessions</div>
        <button
          className="button new-session"
          onClick={() => openDialog({ type: "launch" })}
        >
          <Plus size={16} />
          新建会话
        </button>
        <nav className="main-nav" aria-label="会话导航">
          <button
            className={view === "all" && !project ? "active" : ""}
            onClick={() => switchView("all")}
          >
            <MessageSquare size={17} />
            <span>全部会话</span>
            <b>{activeSessions.length}</b>
          </button>
          <button
            className={view === "starred" ? "active" : ""}
            onClick={() => switchView("starred")}
          >
            <Star size={17} />
            <span>我的收藏</span>
            <b>{activeSessions.filter((s) => s.starred).length}</b>
          </button>
          <button
            className={view === "archived" ? "active" : ""}
            onClick={() => switchView("archived")}
          >
            <Archive size={17} />
            <span>已归档</span>
            <b>{sessions.filter((s) => s.archived).length}</b>
          </button>
        </nav>
        <div className="nav-label project-label">
          <span>项目</span>
          <span>{projects.length}</span>
        </div>
        <nav className="project-nav" aria-label="项目筛选">
          {projects.map(([cwd, item]) => (
            <button
              key={cwd}
              title={cwd}
              className={project === cwd ? "active" : ""}
              onClick={() => switchView("all", cwd)}
            >
              <Folder size={14} />
              <span>{item.name}</span>
              <small>{item.count}</small>
            </button>
          ))}
          {!projects.length && (
            <p className="no-projects">读取会话后，项目会自动出现在这里。</p>
          )}
        </nav>
        <div className="sidebar-bottom">
          <button
            className="settings-button"
            onClick={() => openDialog({ type: "settings" })}
          >
            <Settings2 size={16} />
            设置
          </button>
          <button
            className="settings-button"
            onClick={() => openDialog({ type: "help" })}
          >
            <Info size={16} />
            使用帮助
          </button>
        </div>
      </aside>
      <main className="main-workspace">
        <div className="workspace-content">
          <header className="page-heading">
            <button
              className="icon-button mobile-menu"
              aria-label="打开导航"
              onClick={() => setSidebarOpen(true)}
            >
              <Menu size={18} />
            </button>
            <h1>
              {project
                ? projects.find(([cwd]) => cwd === project)?.[1].name
                : titles[view]}
            </h1>
            {config?.demo && <span className="demo-badge">演示模式</span>}
            <div className="spacer" />
            <button
              className="icon-button"
              title={dark ? "切换浅色模式" : "切换深色模式"}
              aria-label={dark ? "切换浅色模式" : "切换深色模式"}
              onClick={() => setDark(!dark)}
            >
              {dark ? <Sun size={17} /> : <Moon size={17} />}
            </button>
            <button
              className="button secondary refresh-button"
              disabled={busy}
              onClick={() => void refresh()}
            >
              <RefreshCw size={14} className={busy ? "spin" : ""} />
              刷新
            </button>
          </header>
          {fatalError && (
            <div className="inline-error" role="alert">
              无法读取会话：{fatalError}。
            </div>
          )}
          {index.warnings.length > 0 && (
            <details className="scan-warnings">
              <summary>
                <Info size={14} />
                {index.warnings.length === 1
                  ? index.warnings[0]
                  : `${index.warnings.length} 条扫描提示`}
                <ChevronDown size={13} />
              </summary>
              {index.warnings.map((warning, i) => (
                <p key={i}>{warning}</p>
              ))}
            </details>
          )}
          <div className="session-workbench">
            <section className="list-panel" aria-label="会话列表">
              <div className="list-search">
                <Search size={17} />
                <input
                  ref={searchRef}
                  value={search}
                  onChange={(event) => setSearch(event.target.value)}
                  placeholder="搜索会话、项目或摘要…"
                  aria-label="搜索会话"
                />
                {search ? (
                  <button
                    className="icon-button"
                    aria-label="清空搜索"
                    onClick={() => setSearch("")}
                  >
                    <X size={13} />
                  </button>
                ) : (
                  <kbd>Ctrl K</kbd>
                )}
              </div>
              <div className="list-toolbar">
                <label className="select-all">
                  <input
                    type="checkbox"
                    checked={allChecked}
                    onChange={() =>
                      setSelected((previous) => {
                        const next = new Set(previous);
                        shown.forEach((s) =>
                          allChecked ? next.delete(s.key) : next.add(s.key),
                        );
                        return next;
                      })
                    }
                    aria-label="选择当前显示的全部会话"
                  />
                  <span>
                    {selected.size
                      ? `已选 ${selected.size} 项`
                      : `${filtered.length} 个会话`}
                  </span>
                </label>
                <div className="sort-control">
                  <ArrowDownWideNarrow size={13} />
                  <select
                    aria-label="会话排序"
                    value={sort}
                    onChange={(event) => setSort(event.target.value)}
                  >
                    <option value="updated">最近更新</option>
                    <option value="name">名称排序</option>
                    <option value="messages">消息数量</option>
                  </select>
                </div>
              </div>
              {selected.size > 0 && (
                <div className="batch-toolbar" aria-label="批量操作">
                  <button
                    title="批量收藏"
                    aria-label="批量收藏"
                    disabled={busy}
                    onClick={() => void batchStar()}
                  >
                    <Star size={15} />
                  </button>
                  <button
                    title="批量重命名"
                    aria-label="批量重命名"
                    onClick={() =>
                      openDialog({ type: "rename", keys: [...selected] })
                    }
                  >
                    <Pencil size={15} />
                  </button>
                  <button
                    title="批量导出"
                    aria-label="批量导出"
                    onClick={() =>
                      openDialog({ type: "export", keys: [...selected] })
                    }
                  >
                    <Download size={15} />
                  </button>
                  <button
                    title={view === "archived" ? "批量恢复" : "批量归档"}
                    aria-label={view === "archived" ? "批量恢复" : "批量归档"}
                    onClick={() =>
                      openDialog({
                        type: "archive",
                        keys: [...selected],
                        restore: view === "archived",
                      })
                    }
                  >
                    <Archive size={15} />
                  </button>
                  <span className="spacer" />
                  <button
                    title="取消选择"
                    aria-label="取消选择"
                    onClick={() => setSelected(new Set())}
                  >
                    <X size={15} />
                  </button>
                </div>
              )}
              <div className="session-list">
                {loading ? (
                  <div className="loading-state">
                    <Loader2 className="spin" size={22} />
                    正在读取本地会话
                  </div>
                ) : shown.length ? (
                  shown.map((session) => (
                    <article
                      className={`session-card ${session.key === selectedKey ? "selected" : ""}`}
                      key={session.key}
                    >
                      <div className="session-card-top">
                        <input
                          type="checkbox"
                          aria-label={`选择 ${session.name}`}
                          checked={selected.has(session.key)}
                          onChange={() => toggleSelection(session.key)}
                        />
                        <span className="session-project">
                          <Folder size={12} />
                          {session.project}
                        </span>
                        <time title={session.updatedAt}>
                          {relativeTime(session.updatedAt)}
                        </time>
                      </div>
                      <button
                        className="session-select"
                        onClick={() => {
                          setSelectedKey(session.key);
                          setMobileDetail(true);
                        }}
                      >
                        <h3>{session.name}</h3>
                        <p>{session.preview}</p>
                        <div className="session-card-meta">
                          <span className="model-label">{session.model}</span>
                          <span className="spacer" />
                          <MessageSquare size={12} />
                          <span>{session.messageCount}</span>
                          {session.branchCount > 0 && (
                            <span title="包含分支">
                              ⑂ {session.branchCount}
                            </span>
                          )}
                        </div>
                      </button>
                      <button
                        className={`card-star ${session.starred ? "is-starred" : ""}`}
                        title={session.starred ? "取消收藏" : "收藏"}
                        aria-label={`${session.starred ? "取消收藏" : "收藏"} ${session.name}`}
                        onClick={() => quickStar(session)}
                      >
                        <Star
                          size={13}
                          fill={session.starred ? "currentColor" : "none"}
                        />
                      </button>
                    </article>
                  ))
                ) : (
                  <div className="list-empty">
                    <h3>
                      {search
                        ? "没有找到相关会话"
                        : view === "starred"
                          ? "还没有收藏的会话"
                          : view === "archived"
                            ? "暂无归档会话"
                            : "暂无会话"}
                    </h3>
                    <p>
                      {search
                        ? "试试其他关键词，或清除搜索条件。"
                        : view === "all"
                          ? "使用 pi 创建会话后，会自动出现在这里。"
                          : "选择会话，即可收藏或归档。"}
                    </p>
                    {search && (
                      <button
                        className="button secondary small"
                        onClick={() => setSearch("")}
                      >
                        清空搜索
                      </button>
                    )}
                    {!search && view === "all" && (
                      <button
                        className="button primary small"
                        onClick={() => openDialog({ type: "launch" })}
                      >
                        <Plus size={14} />
                        新建会话
                      </button>
                    )}
                  </div>
                )}
                {filtered.length > pageSize && (
                  <button
                    className="load-more"
                    onClick={() => setPageSize(pageSize + 100)}
                  >
                    加载更多会话
                  </button>
                )}
              </div>
              <div className="list-footer" title="自动读取 pi 已保存的会话记录">
                {connected ? "自动更新" : "更新暂停"}
              </div>
            </section>
            <Detail
              session={current}
              revision={revision}
              notify={notify}
              onRename={(keys) => openDialog({ type: "rename", keys })}
              onStar={quickStar}
              onArchive={(session) =>
                openDialog({
                  type: "archive",
                  keys: [session.key],
                  restore: session.archived,
                })
              }
              onLaunch={(session) => openDialog({ type: "launch", session })}
              onExport={(keys) => openDialog({ type: "export", keys })}
              onBack={() => setMobileDetail(false)}
            />
          </div>
        </div>
      </main>
      {toast && (
        <div
          className={`toast ${toast.error ? "toast-error" : ""}`}
          role={toast.error ? "alert" : "status"}
        >
          {toast.error ? <Info size={18} /> : <CheckCheck size={18} />}
          <span>{toast.text}</span>
          <button aria-label="关闭通知" onClick={() => setToast(undefined)}>
            <X size={15} />
          </button>
        </div>
      )}
      {dialog && (
        <Modal
          title={
            dialog.type === "rename"
              ? dialog.keys.length === 1
                ? "重命名会话"
                : `批量重命名 ${dialog.keys.length} 个会话`
              : dialog.type === "archive"
                ? dialog.restore
                  ? "恢复会话"
                  : "归档会话"
                : dialog.type === "launch"
                  ? dialog.session
                    ? "在终端继续会话"
                    : "开启新的会话"
                  : dialog.type === "export"
                    ? "导出会话"
                    : dialog.type === "settings"
                      ? "设置"
                      : "使用帮助"
          }
          onClose={closeDialog}
          wide={dialog.type === "settings"}
        >
          {dialog.type === "settings" ? (
            <div className="settings-content">
              <div className="setting-field">
                <label>PI 会话目录</label>
                <code>{config?.sessionRoot || "正在读取…"}</code>
                <p>
                  默认读取 pi 的会话目录；也可以选择使用 --session-dir
                  创建的自定义目录。
                </p>
                <div className="setting-actions">
                  <button
                    className="button secondary small"
                    disabled={busy || config?.demo}
                    onClick={() => void changeRoot()}
                  >
                    <FolderOpen size={14} />
                    选择会话目录
                  </button>
                  <button
                    className="button subtle small"
                    disabled={busy || config?.demo}
                    onClick={() => void changeRoot(true)}
                  >
                    恢复默认
                  </button>
                </div>
              </div>
              <div className="setting-field">
                <label>管理器数据目录</label>
                <code>{config?.dataDir || "正在读取…"}</code>
                <p>SQLite 保存名称、收藏、归档和应用设置。不改写会话内容。</p>
              </div>
              <div className="setting-field">
                <label htmlFor="default-terminal">默认终端</label>
                <select
                  id="default-terminal"
                  className="text-input"
                  value={selectedTerminal}
                  disabled={busy || !config}
                  onChange={(event) =>
                    void changeTerminal(
                      event.target.value as TerminalPreference,
                    )
                  }
                >
                  {!selectedTerminal && (
                    <option value="" disabled>
                      {config ? "未检测到可用终端" : "正在检测…"}
                    </option>
                  )}
                  {config?.terminalOptions.map((terminal) => (
                    <option
                      key={terminal.id}
                      value={terminal.id}
                      disabled={!terminal.available}
                    >
                      {terminal.label}
                    </option>
                  ))}
                </select>
                <p>
                  当前使用：{config?.terminal || "正在检测…"}。选择后自动保存。
                </p>
                <p>
                  需要在 PATH 中安装 pi。自定义 pi 入口使用
                  PI_SESSION_MANAGER_PI_BIN。
                </p>
              </div>
              <div className="setting-field">
                <label>外观</label>
                <button
                  className="button secondary small"
                  onClick={() => setDark(!dark)}
                >
                  {dark ? <Sun size={15} /> : <Moon size={15} />}
                  {dark ? "切换为浅色" : "切换为深色"}
                </button>
              </div>
              <div className="info-note">
                <p>
                  自动更新仅显示 pi 已保存的记录，不包含尚未写入文件的内容。
                </p>
              </div>
            </div>
          ) : dialog.type === "help" ? (
            <div className="help-content">
              <p>
                <kbd>Ctrl / ⌘ K</kbd> 快速搜索会话
              </p>
              <p>
                <kbd>?</kbd> 打开帮助
              </p>
              <p>
                <kbd>Esc</kbd> 关闭弹窗
              </p>
              <hr />
              <p>勾选会话后，可以批量收藏、归档、重命名和导出。</p>
              <p>
                重命名模板支持 <code>{"{name}"}</code> 原名称和{" "}
                <code>{"{n}"}</code> 序号。
              </p>
              <p>
                “最后持久化分支”按最近一条记录回溯历史，不代表 pi
                进程中尚未保存的树位置，也不是压缩后的模型上下文。
              </p>
              <p>
                正在使用的会话建议“分叉后继续”，避免两个 pi
                进程同时写入同一个文件。
              </p>
            </div>
          ) : (
            <form
              onSubmit={(event) => {
                event.preventDefault();
                void confirmDialog();
              }}
            >
              <div className="modal-body">
                {dialog.type === "rename" && (
                  <>
                    <label className="form-label" htmlFor="session-name">
                      {dialog.keys.length > 1 ? "名称模板" : "会话名称"}
                    </label>
                    <input
                      autoFocus
                      id="session-name"
                      className="text-input"
                      maxLength={160}
                      value={name}
                      onChange={(event) => setName(event.target.value)}
                      placeholder="输入会话名称"
                      required
                    />
                    <p className="form-hint">
                      {dialog.keys.length > 1
                        ? "使用 {name} 保留原名称，{n} 添加从 1 开始的序号。例如：调试记录 {n}"
                        : "名称先保存在管理器中，下次从页面恢复会话时同步到 pi。"}
                    </p>
                    {dialog.keys.length > 1 && (
                      <div className="rename-preview">
                        {dialog.keys.slice(0, 3).map((key, i) => (
                          <p key={key}>
                            <ChevronRight size={12} />
                            {name
                              .replace(
                                /\{name\}/g,
                                () =>
                                  sessions.find((s) => s.key === key)?.name ||
                                  "",
                              )
                              .replace(/\{n\}/g, String(i + 1)) || "（空名称）"}
                          </p>
                        ))}
                      </div>
                    )}
                  </>
                )}
                {dialog.type === "archive" && (
                  <>
                    <p>
                      将 {dialog.keys.length} 个会话
                      {dialog.restore ? "恢复到会话列表" : "移入归档列表"}？
                    </p>
                    <p className="form-hint">
                      归档只改变管理器的显示状态，不会移动或删除文件，也不影响终端中正在进行的对话。
                    </p>
                  </>
                )}
                {dialog.type === "export" && (
                  <>
                    <p>导出 {dialog.keys.length} 个会话的完整原始记录。</p>
                    <p className="form-hint">
                      {dialog.keys.length === 1
                        ? "使用系统保存对话框导出 JSONL，可以通过 pi --session 恢复。"
                        : "使用系统保存对话框导出 JSON 包，包含各会话的 JSONL 文本和管理信息。"}
                      会话可能包含源码、终端输出和凭据，请妥善保存，不要公开分享。
                    </p>
                  </>
                )}
                {dialog.type === "launch" && (
                  <>
                    <div className="launch-summary">
                      <div>
                        <strong>{dialog.session?.name || "新建会话"}</strong>
                        <p>{config?.terminal || "系统终端"} · pi</p>
                      </div>
                    </div>
                    {dialog.session ? (
                      <>
                        <label className="form-label">工作目录</label>
                        <code className="cwd-display">
                          {dialog.session.cwd}
                        </code>
                        <label className="fork-option">
                          <input
                            type="checkbox"
                            checked={fork}
                            onChange={(event) => setFork(event.target.checked)}
                          />
                          <div>
                            <strong>分叉后继续</strong>
                            <span>创建新会话，保留原始记录不变</span>
                          </div>
                        </label>
                        <p className="form-hint warning-hint">
                          无法可靠检测其他终端中是否正在运行此会话。若会话仍在使用，请启用分叉，避免并发写入。
                        </p>
                      </>
                    ) : (
                      <>
                        <label className="form-label" htmlFor="new-project">
                          工作目录
                        </label>
                        <select
                          className="text-input"
                          id="new-project"
                          value={projectKey}
                          onChange={(event) =>
                            setProjectKey(event.target.value)
                          }
                        >
                          <option value="">
                            当前目录 · {config?.defaultCwd}
                          </option>
                          {projects.map(([cwd, item]) => (
                            <option key={cwd} value={item.key}>
                              {item.name} · {cwd}
                            </option>
                          ))}
                        </select>
                        <div className="workspace-picker">
                          <button
                            type="button"
                            className="button secondary small"
                            onClick={() => void chooseWorkspace()}
                          >
                            <FolderOpen size={14} />
                            选择其他工作目录
                          </button>
                          {newCwd && <code>{newCwd}</code>}
                        </div>
                        <p className="form-hint">
                          在系统终端中启动 pi，完成对话后会自动出现在列表中。
                        </p>
                      </>
                    )}
                    {config?.demo && (
                      <div className="inline-warning">
                        演示模式不会启动真实终端。请退出后以普通模式启动 Pi
                        Sessions。
                      </div>
                    )}
                  </>
                )}
                {dialogError && (
                  <p className="form-error" role="alert">
                    {dialogError}
                  </p>
                )}
              </div>
              <div className="modal-actions">
                <button
                  type="button"
                  className="button secondary"
                  disabled={busy}
                  onClick={closeDialog}
                >
                  取消
                </button>
                <button
                  type="submit"
                  className="button primary"
                  disabled={
                    busy ||
                    (dialog.type === "rename" && !name.trim()) ||
                    (dialog.type === "launch" && config?.demo)
                  }
                >
                  {busy && <Loader2 size={15} className="spin" />}
                  {dialog.type === "rename"
                    ? "保存名称"
                    : dialog.type === "archive"
                      ? dialog.restore
                        ? "恢复会话"
                        : "确认归档"
                      : dialog.type === "export"
                        ? "选择保存位置"
                        : fork
                          ? "分叉并打开终端"
                          : "打开终端"}
                </button>
              </div>
            </form>
          )}
        </Modal>
      )}
    </div>
  );
}
