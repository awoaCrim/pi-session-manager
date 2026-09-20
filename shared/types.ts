export interface SessionMeta {
  name?: string;
  starred?: boolean;
  archived?: boolean;
}
export interface SessionSummary {
  key: string;
  id: string;
  name: string;
  nativeName: string | null;
  cwd: string;
  project: string;
  path: string;
  createdAt: string;
  updatedAt: string;
  preview: string;
  model: string;
  provider: string;
  messageCount: number;
  tokens: number;
  cost: number;
  fileSize: number;
  branchCount: number;
  starred: boolean;
  archived: boolean;
  malformedLines: number;
  version: number;
}
export interface ContentBlock {
  type: "text" | "thinking" | "toolCall" | "image";
  text?: string;
  name?: string;
  arguments?: string;
  mimeType?: string;
  truncated?: boolean;
}
export interface SessionEntry {
  id: string;
  parentId: string | null;
  type: string;
  role: string;
  timestamp: string;
  content: ContentBlock[];
  model?: string;
  toolName?: string;
  isError?: boolean;
  tokens?: number;
}
export interface SessionDetail {
  session: SessionSummary;
  entries: SessionEntry[];
  total: number;
  hasMore: boolean;
  branch: "all" | "active";
  warning?: string;
}
export interface SessionIndex {
  sessions: SessionSummary[];
  warnings: string[];
  scannedAt: string;
}
export type TerminalPreference = "auto" | "pwsh" | "powershell" | "system";
export interface TerminalOption {
  id: TerminalPreference;
  label: string;
  available: boolean;
}
export interface Bootstrap {
  version: string;
  sessionRoot: string;
  dataDir: string;
  platform: string;
  terminal: string;
  terminalPreference: TerminalPreference;
  terminalOptions: TerminalOption[];
  demo: boolean;
  defaultCwd: string;
  pollInterval: number;
}
export interface BatchRequest {
  keys: string[];
  action: "star" | "unstar" | "archive" | "restore" | "rename";
  name?: string;
}
