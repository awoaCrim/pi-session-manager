import { useEffect, useRef, useState } from "react";
import {
  Archive,
  ArrowDown,
  ArrowLeft,
  Check,
  ChevronDown,
  Code2,
  Copy,
  Download,
  Folder,
  GitBranch,
  Image,
  Info,
  Loader2,
  MessageSquare,
  Pencil,
  Search,
  Sparkles,
  Star,
  Terminal,
  X,
} from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type {
  ContentBlock,
  SessionDetail,
  SessionEntry,
  SessionSummary,
} from "../shared/types";
import { api, formatDate, formatNumber } from "./api";

type Props = {
  session: SessionSummary | undefined;
  revision: number;
  notify: (message: string, error?: boolean) => void;
  onRename: (keys: string[]) => void;
  onStar: (session: SessionSummary) => void;
  onArchive: (session: SessionSummary) => void;
  onLaunch: (session: SessionSummary) => void;
  onExport: (keys: string[]) => void;
  onBack: () => void;
};
function CopyButton({
  text,
  notify,
}: {
  text: string;
  notify: Props["notify"];
}) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="icon-button copy-button"
      title="复制内容"
      aria-label="复制内容"
      onClick={() =>
        void navigator.clipboard
          .writeText(text)
          .then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          })
          .catch(() => notify("复制失败，请手动选择内容", true))
      }
    >
      {copied ? <Check size={14} /> : <Copy size={14} />}
    </button>
  );
}
function Content({
  block,
  notify,
}: {
  block: ContentBlock;
  notify: Props["notify"];
}) {
  if (block.type === "thinking")
    return (
      <details className="thinking-block">
        <summary>
          <Sparkles size={14} /> 思考过程 <ChevronDown size={13} />
        </summary>
        <div className="plain-content">
          {block.text}
          {block.truncated && (
            <p className="truncated">内容较长，已截断。完整内容请导出查看。</p>
          )}
        </div>
      </details>
    );
  if (block.type === "toolCall")
    return (
      <details className="tool-block">
        <summary>
          <Terminal size={14} />
          <b>{block.name}</b>
          <span>工具调用</span>
          <ChevronDown size={13} />
        </summary>
        <pre>
          <code>{block.arguments}</code>
        </pre>
        {block.truncated && <p className="truncated">参数过长，已截断</p>}
      </details>
    );
  if (block.type === "image")
    return (
      <div className="image-placeholder">
        <Image size={18} />
        <span>
          图片附件 · {block.mimeType || "image"}
          <small>为减少内存占用，请在 pi 中查看原图</small>
        </span>
      </div>
    );
  return (
    <div className="markdown">
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          img: ({ alt }) => (
            <span className="inline-image">
              [图片：{alt || "已阻止外部图片加载"}]
            </span>
          ),
          a: ({ href, children }) => (
            <a
              href={href}
              onClick={(event) => {
                event.preventDefault();
                if (href)
                  void api
                    .openExternal(href)
                    .catch(() => notify("无法打开外部链接", true));
              }}
            >
              {children}
            </a>
          ),
          pre: ({ children }) => <pre>{children}</pre>,
        }}
      >
        {block.text || ""}
      </Markdown>
      {block.truncated && (
        <p className="truncated">显示前 32,000 字符，完整内容请导出查看。</p>
      )}
    </div>
  );
}
function Message({
  entry,
  notify,
}: {
  entry: SessionEntry;
  notify: Props["notify"];
}) {
  const [raw, setRaw] = useState(false);
  if (entry.role === "system")
    return (
      <details className="system-entry">
        <summary>
          <GitBranch size={12} />
          <span>
            {entry.type === "compaction"
              ? "上下文压缩"
              : entry.type === "branch_summary"
                ? "分支摘要"
                : entry.type === "custom"
                  ? "扩展记录"
                  : entry.content[0]?.text?.slice(0, 100) || entry.type}
          </span>
          <ChevronDown size={12} />
        </summary>
        <pre>{entry.content.map((c) => c.text).join("\n")}</pre>
      </details>
    );
  const tool = entry.role === "toolResult" || entry.role === "bashExecution";
  if (tool)
    return (
      <details className={`tool-result ${entry.isError ? "tool-error" : ""}`}>
        <summary>
          <Terminal size={14} />
          <b>{entry.toolName || "bash"}</b>
          <span>{entry.isError ? "执行失败" : "执行结果"}</span>
          {entry.isError ? <X size={13} /> : <Check size={13} />}
          <ChevronDown size={13} />
        </summary>
        <div>
          {entry.content.map((block, i) =>
            block.type === "text" ? (
              <pre key={i}>
                {block.text}
                {block.truncated ? "\n…（已截断）" : ""}
              </pre>
            ) : (
              <Content key={i} block={block} notify={notify} />
            ),
          )}
        </div>
      </details>
    );
  const user = entry.role === "user";
  return (
    <article
      className={`message ${user ? "user-message" : "assistant-message"} ${entry.isError ? "message-error" : ""}`}
    >
      <div className="message-main">
        <div className="message-meta">
          <strong>
            {user ? "你" : entry.role === "assistant" ? "Pi" : "扩展消息"}
          </strong>
          {!user && entry.model && (
            <span className="message-model">{entry.model}</span>
          )}
          <time>
            {entry.timestamp
              ? new Date(entry.timestamp).toLocaleTimeString("zh-CN", {
                  hour: "2-digit",
                  minute: "2-digit",
                })
              : ""}
          </time>
          <div className="message-actions">
            <button
              className="icon-button"
              title={raw ? "显示消息" : "查看记录 JSON"}
              aria-label="查看记录 JSON"
              onClick={() => setRaw(!raw)}
            >
              <Code2 size={14} />
            </button>
            <CopyButton
              text={entry.content
                .map((c) => c.text || c.arguments || "")
                .join("\n")}
              notify={notify}
            />
          </div>
        </div>
        {raw ? (
          <pre className="raw-record">{JSON.stringify(entry, null, 2)}</pre>
        ) : (
          <div className="message-content">
            {entry.content.map((block, i) => (
              <Content key={i} block={block} notify={notify} />
            ))}
          </div>
        )}
      </div>
    </article>
  );
}
export function Detail({
  session,
  revision,
  notify,
  onRename,
  onStar,
  onArchive,
  onLaunch,
  onExport,
  onBack,
}: Props) {
  const [detail, setDetail] = useState<SessionDetail>();
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [tab, setTab] = useState<"messages" | "info">("messages");
  const [branch, setBranch] = useState<"all" | "active">("all");
  const [limit, setLimit] = useState(100);
  const [tools, setTools] = useState(true);
  const [filter, setFilter] = useState("");
  const [follow, setFollow] = useState(true);
  const scroller = useRef<HTMLDivElement>(null);
  const bottom = useRef<HTMLDivElement>(null);
  const key = session?.key;
  useEffect(() => {
    setDetail(undefined);
    setLimit(100);
    setBranch("all");
    setFilter("");
    setFollow(true);
    setTab("messages");
  }, [key]);
  useEffect(() => {
    if (!key) return;
    const controller = new AbortController();
    setLoading(true);
    setError("");
    api
      .detail(key, branch, limit, controller.signal)
      .then(setDetail)
      .catch((error) => {
        if (error.name !== "AbortError") setError(error.message);
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [key, branch, limit, revision]);
  useEffect(() => {
    if (follow && tab === "messages")
      bottom.current?.scrollIntoView({ behavior: "instant", block: "end" });
  }, [detail, follow, tab]);
  if (!session)
    return (
      <section className="detail-panel empty-detail">
        <h2>选择一个会话</h2>
        <p>从左侧列表选择会话查看记录。</p>
      </section>
    );
  const entries = (
    detail && detail.session.key === key ? detail.entries : []
  ).filter((entry) => {
    if (
      !tools &&
      (entry.role === "toolResult" ||
        entry.role === "bashExecution" ||
        entry.role === "system" ||
        (entry.content.length > 0 &&
          entry.content.every(
            (c) => c.type === "toolCall" || c.type === "thinking",
          )))
    )
      return false;
    return (
      !filter ||
      [
        entry.role,
        entry.toolName,
        ...entry.content.map((c) => c.text || c.arguments || c.name),
      ]
        .join(" ")
        .toLowerCase()
        .includes(filter.toLowerCase())
    );
  });
  const fields = [
    ["会话 ID", session.id],
    ["工作目录", session.cwd],
    ["会话文件", session.path],
    ["Pi 原生名称", session.nativeName || "尚未命名"],
    [
      "模型",
      `${session.provider ? `${session.provider} / ` : ""}${session.model}`,
    ],
    ["创建时间", formatDate(session.createdAt)],
    ["最后写入", formatDate(session.updatedAt)],
    ["文件大小", `${(session.fileSize / 1024).toFixed(1)} KB`],
    ["会话格式", `JSONL · v${session.version}`],
    ["累计 Tokens", formatNumber(session.tokens)],
    ["记录的费用", "$" + session.cost.toFixed(4)],
    ["分支节点", String(session.branchCount)],
  ];
  return (
    <section className="detail-panel">
      <header className="detail-header">
        <div className="detail-title-row">
          <button
            className="icon-button mobile-back"
            aria-label="返回会话列表"
            onClick={onBack}
          >
            <ArrowLeft size={18} />
          </button>
          <div className="detail-heading">
            <h2 title={session.name}>{session.name}</h2>
            <div>
              <Folder size={12} />
              <span>{session.project}</span>
              <span className="dot-separator">·</span>
              <span>{formatNumber(session.messageCount)} 条消息</span>
            </div>
          </div>
          <button
            className={`icon-button ${session.starred ? "is-starred" : ""}`}
            title="收藏会话"
            aria-label="收藏会话"
            onClick={() => onStar(session)}
          >
            <Star size={17} fill={session.starred ? "currentColor" : "none"} />
          </button>
          <button
            className="icon-button"
            title="重命名"
            aria-label="重命名会话"
            onClick={() => onRename([session.key])}
          >
            <Pencil size={16} />
          </button>
          <button
            className="button subtle small"
            onClick={() => onExport([session.key])}
          >
            <Download size={14} />
            导出
          </button>
          <button
            className="button primary small"
            onClick={() => onLaunch(session)}
          >
            <Terminal size={15} />
            在终端继续
          </button>
        </div>
      </header>
      <div className="detail-tabs">
        <button
          className={tab === "messages" ? "active" : ""}
          onClick={() => setTab("messages")}
        >
          <MessageSquare size={14} />
          对话记录
        </button>
        <button
          className={tab === "info" ? "active" : ""}
          onClick={() => setTab("info")}
        >
          <Info size={14} />
          会话信息
        </button>
        <span className="spacer" />
        <span className="record-count">{detail?.total ?? "—"} 条记录</span>
      </div>
      {tab === "info" ? (
        <div className="session-info">
          <dl>
            {fields.map(([label, value]) => (
              <div key={label}>
                <dt>{label}</dt>
                <dd>
                  {value}
                  <CopyButton text={value} notify={notify} />
                </dd>
              </div>
            ))}
          </dl>
          <div className="info-note">
            <Info size={17} />
            <p>
              显示名称与归档状态保存在管理器中。从此页面恢复会话时，会通过{" "}
              <code>--name</code> 同步名称到 pi。归档不会移动或删除原始文件。
            </p>
          </div>
          <button
            className="button secondary"
            onClick={() => onArchive(session)}
          >
            <Archive size={15} />
            {session.archived ? "取消归档" : "归档此会话"}
          </button>
        </div>
      ) : (
        <>
          <div className="message-toolbar">
            <select
              aria-label="分支视图"
              value={branch}
              onChange={(event) =>
                setBranch(event.target.value as "all" | "active")
              }
            >
              <option value="all">全部分支记录</option>
              <option value="active">最后持久化分支</option>
            </select>
            <label className="toggle-label">
              <input
                type="checkbox"
                checked={tools}
                onChange={(event) => setTools(event.target.checked)}
              />
              <span className="toggle" />
              工具与事件
            </label>
            <div className="spacer" />
            <div className="message-search">
              <Search size={13} />
              <input
                aria-label="搜索已加载消息"
                placeholder="查找消息"
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
              />
            </div>
          </div>
          {error && <div className="inline-error">{error}</div>}
          {detail?.warning && (
            <div className="inline-warning">{detail.warning}</div>
          )}
          <div
            className="conversation"
            ref={scroller}
            onScroll={() => {
              const el = scroller.current;
              if (el && el.scrollHeight - el.scrollTop - el.clientHeight > 140)
                setFollow(false);
            }}
          >
            {loading && !detail ? (
              <div className="loading-state">
                <Loader2 size={21} className="spin" />
                读取会话中
              </div>
            ) : (
              <>
                <div className="conversation-date">
                  <span />
                  {session.createdAt
                    ? new Date(session.createdAt).toLocaleDateString("zh-CN", {
                        year: "numeric",
                        month: "long",
                        day: "numeric",
                      })
                    : "会话记录"}
                  <span />
                </div>
                {detail?.hasMore && (
                  <button
                    className="load-more"
                    disabled={loading || limit >= 5000}
                    onClick={() => {
                      setFollow(false);
                      setLimit(Math.min(limit + 200, 5000));
                    }}
                  >
                    {limit >= 5000
                      ? "已显示 5000 条，更多内容请导出查看"
                      : loading
                        ? "正在加载…"
                        : `加载更早记录（当前 ${detail.entries.length} / ${detail.total}）`}
                  </button>
                )}
                {filter && (
                  <div className="filter-note">
                    在已加载记录中找到 {entries.length} 条匹配
                  </div>
                )}
                {entries.map((entry) => (
                  <Message key={entry.id} entry={entry} notify={notify} />
                ))}
                {!entries.length && (
                  <div className="message-empty">
                    <MessageSquare size={26} />
                    <p>
                      {filter
                        ? "没有匹配的消息，试试其他关键词"
                        : "暂无消息。可以在终端继续此会话。"}
                    </p>
                  </div>
                )}
                <div ref={bottom} className="conversation-end" />
              </>
            )}
          </div>
          <div className="detail-footer">
            <span className="saved-time">
              最后保存 {formatDate(session.updatedAt)}
            </span>
            <button
              className={`follow-button ${follow ? "following" : ""}`}
              onClick={() => {
                setFollow(!follow);
                if (!follow)
                  bottom.current?.scrollIntoView({ behavior: "smooth" });
              }}
            >
              <ArrowDown size={13} />
              {follow ? "自动跟随" : "回到最新"}
            </button>
          </div>
        </>
      )}
    </section>
  );
}
