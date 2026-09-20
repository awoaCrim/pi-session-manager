import {
  test,
  expect,
  chromium,
  type Browser,
  type Page,
} from "@playwright/test";
import { execFile, spawn, type ChildProcess } from "node:child_process";
import { promisify } from "node:util";
import {
  mkdtemp,
  mkdir,
  writeFile,
  readFile,
  appendFile,
  rm,
  access,
  realpath,
  open,
  rename,
} from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import type {
  Bootstrap,
  SessionDetail,
  SessionIndex,
  TerminalPreference,
} from "../shared/types";

let app: ChildProcess;
let browser: Browser;
let page: Page;
let temp: string;
let root: string;
let original: string;
let file: string;
let terminalPid: number | undefined;
const port = 19226;
const binary =
  process.env.PI_DESKTOP_BINARY ||
  path.resolve("src-tauri/target/debug/pi-session-manager.exe");
const execFileAsync = promisify(execFile);
async function desktopWindow(action: string, menuText?: string) {
  const { stdout } = await execFileAsync(
    "powershell",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      path.resolve("scripts/test-desktop-window.ps1"),
      "-ProcessId",
      String(app.pid),
      "-Action",
      action,
      ...(menuText ? ["-MenuText", menuText] : []),
    ],
    { timeout: 20_000, encoding: "utf8" },
  );
  return stdout.trim() ? JSON.parse(stdout.replace(/^\uFEFF/, "")) : undefined;
}
async function expectWindow(expected: Record<string, unknown>) {
  await expect.poll(() => desktopWindow("State")).toMatchObject(expected);
}
async function command<T = unknown>(name: string, args?: object): Promise<T> {
  return page.evaluate(
    async ({ name, args }) =>
      (window as any).__TAURI_INTERNALS__.invoke(name, args),
    { name, args },
  );
}
async function chooseTerminal(preference: TerminalPreference) {
  await page.getByRole("button", { name: "设置", exact: true }).click();
  const select = page.getByLabel("默认终端", { exact: true });
  await select.selectOption(preference);
  await expect(select).toBeEnabled();
  await expect(select).toHaveValue(preference);
  await expect
    .poll(
      async () => (await command<Bootstrap>("bootstrap")).terminalPreference,
    )
    .toBe(preference);
  await page.getByRole("button", { name: "关闭弹窗" }).click();
}
async function terminalRecord() {
  const recordFile = path.join(temp, "terminal-record.json");
  let record: any;
  await expect
    .poll(
      async () => {
        try {
          record = JSON.parse(
            (await readFile(recordFile, "utf8")).replace(/^\uFEFF/, ""),
          );
          return true;
        } catch {
          return false;
        }
      },
      { timeout: 15_000 },
    )
    .toBe(true);
  terminalPid = record.pid;
  return record;
}
async function stopTerminal() {
  if (!terminalPid) return;
  await new Promise<void>((resolve) => {
    const child = spawn("taskkill", ["/PID", String(terminalPid), "/T", "/F"]);
    child.once("exit", () => resolve());
  });
  terminalPid = undefined;
  await rm(path.join(temp, "terminal-record.json"), { force: true });
}
async function startApp(expectedTitle = "原生会话 A") {
  await access(binary);
  app = spawn(binary, [], {
    stdio: "pipe",
    env: {
      ...process.env,
      PI_CODING_AGENT_SESSION_DIR: root,
      PI_SESSION_MANAGER_DATA_DIR: path.join(temp, "data"),
      PI_SESSION_MANAGER_PI_BIN: path.join(temp, "fake-pi.ps1"),
      WEBVIEW2_USER_DATA_FOLDER: path.join(temp, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    },
  });
  let logs = "";
  app.stderr?.on("data", (chunk) => {
    logs += chunk;
  });
  const until = Date.now() + 35_000;
  while (Date.now() < until) {
    if (app.exitCode !== null)
      throw new Error(`桌面程序提前退出。请关闭其他 Pi Sessions 实例。${logs}`);
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {
        timeout: 1000,
      });
      const pages = browser.contexts().flatMap((c) => c.pages());
      const candidate = pages.find((p) => p.url().includes("tauri.localhost"));
      if (candidate) {
        page = candidate;
        await expect(
          page.getByRole("heading", {
            name: expectedTitle,
            exact: true,
            level: 2,
          }),
        ).toBeVisible();
        await expectWindow({ visible: true, minimized: false });
        return;
      }
      await browser.close();
    } catch {
      /* Wait for WebView2 initialization. */
    }
    await new Promise((resolve) => setTimeout(resolve, 400));
  }
  throw new Error(`WebView2 调试窗口没有启动。${logs}`);
}
async function stopApp() {
  await browser?.close().catch(() => {});
  if (app && app.exitCode === null) {
    if (process.platform === "win32")
      await new Promise<void>((resolve) => {
        const kill = spawn("taskkill", ["/PID", String(app.pid), "/T", "/F"]);
        kill.once("exit", () => resolve());
      });
    else app.kill("SIGTERM");
  }
}
test.describe
  .serial("Tauri desktop with real Rust IPC and isolated Pi files", () => {
  test.beforeAll(async () => {
    test.setTimeout(60_000);
    if (process.platform !== "win32")
      throw new Error("此原生 E2E 脚本需要 Windows WebView2");
    temp = await mkdtemp(path.join(os.tmpdir(), "pi-desktop-e2e-"));
    const recordPath = path
      .join(temp, "terminal-record.json")
      .replace(/'/g, "''");
    await writeFile(
      path.join(temp, "fake-pi.ps1"),
      `@{ arguments=@($args); cwd=(Get-Location).Path; pid=$PID; majorVersion=$PSVersionTable.PSVersion.Major; executable=(Get-Process -Id $PID).Path; inputRedirected=[Console]::IsInputRedirected; outputRedirected=[Console]::IsOutputRedirected } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath '${recordPath}' -Encoding UTF8`,
    );
    root = path.join(temp, "sessions");
    await mkdir(root);
    const cwd = path.join(temp, "workspace");
    await mkdir(cwd);
    const header = {
      type: "session",
      version: 3,
      id: "native-e2e-a",
      cwd,
      timestamp: new Date().toISOString(),
    };
    const rows = [
      header,
      {
        type: "session_info",
        id: "name-a",
        parentId: null,
        name: "原生会话 A",
      },
      {
        type: "message",
        id: "a1",
        parentId: "name-a",
        timestamp: new Date().toISOString(),
        message: { role: "user", content: "帮我检查桌面会话管理功能" },
      },
      {
        type: "message",
        id: "a2",
        parentId: "a1",
        timestamp: new Date().toISOString(),
        message: {
          role: "assistant",
          model: "test-model",
          provider: "test",
          content: [
            { type: "thinking", thinking: "先检查 IPC 再测试文件监听" },
            {
              type: "text",
              text: "原生 Rust 已读取消息。\n\n```ts\nconst desktop = true;\n```",
            },
          ],
          usage: { totalTokens: 100, cost: { total: 0.001 } },
        },
      },
    ];
    original = rows.map((r) => JSON.stringify(r)).join("\n") + "\n";
    file = path.join(root, "a.jsonl");
    await writeFile(file, original);
    const b = [
      { ...header, id: "native-e2e-b" },
      {
        type: "session_info",
        id: "name-b",
        parentId: null,
        name: "原生会话 B",
      },
      {
        type: "message",
        id: "b1",
        parentId: "name-b",
        message: { role: "user", content: "另一个本地会话" },
      },
    ];
    await writeFile(
      path.join(root, "b.jsonl"),
      b.map((r) => JSON.stringify(r)).join("\n") + "\n",
    );
    // Ensure initial list selects A even on file systems with coarse mtime resolution.
    const { utimes } = await import("node:fs/promises");
    await utimes(path.join(root, "b.jsonl"), new Date(0), new Date(0));
    await startApp();
  });
  test.afterAll(async () => {
    await stopApp();
    await stopTerminal();
    if (temp)
      await rm(temp, {
        recursive: true,
        force: true,
        maxRetries: 5,
        retryDelay: 300,
      }).catch(() => {});
  });
  test("opens a real desktop window and reads native session details", async () => {
    await expect(page).toHaveTitle(/Pi Sessions/);
    await expect(page.locator(".list-footer")).toHaveText("自动更新");
    await expect(
      page.locator(
        ".top-avatar, .stats-row, .eyebrow, .local-card, .workspace-footer, .avatar",
      ),
    ).toHaveCount(0);
    await expect(page.getByText("YOUR CONVERSATIONS, CONNECTED")).toHaveCount(
      0,
    );
    await expect(
      page.getByText("原生 Rust 已读取消息。", { exact: true }),
    ).toBeVisible();
    await page.locator(".thinking-block summary").click();
    await expect(page.getByText("先检查 IPC 再测试文件监听")).toBeVisible();
    const config = await command<any>("bootstrap");
    expect(config.platform).toBe("windows");
    expect(config.sessionRoot).toBe(root);
    expect(config.terminalPreference).toBe("auto");
    const hasPwsh = config.terminalOptions.some(
      (option: any) => option.id === "pwsh",
    );
    expect(config.terminal).toBe(
      hasPwsh ? "PowerShell 7" : "Windows PowerShell",
    );
    await page.screenshot({ path: "test-results/desktop-initial.png" });
  });
  test("search preserves the selected conversation and keyboard shortcut focuses search", async () => {
    await page.keyboard.press("Control+k");
    await expect(
      page.getByRole("textbox", { name: "搜索会话", exact: true }),
    ).toBeFocused();
    await page
      .getByRole("textbox", { name: "搜索会话", exact: true })
      .fill("会话 B");
    await expect(page.locator(".session-card")).toHaveCount(1);
    await expect(
      page.getByRole("heading", { name: "原生会话 A", level: 2 }),
    ).toBeVisible();
    await page.getByRole("button", { name: "清空搜索", exact: true }).click();
  });
  test("renames through SQLite without modifying the source JSONL", async () => {
    await page.getByRole("button", { name: "重命名会话", exact: true }).click();
    await page.getByLabel("会话名称", { exact: true }).fill("桌面端重命名验证");
    await page.getByRole("button", { name: "保存名称" }).click();
    await expect(
      page.getByRole("heading", { name: "桌面端重命名验证", level: 2 }),
    ).toBeVisible();
    expect(await readFile(file, "utf8")).toBe(original);
  });
  test("exports the original JSONL through the real Windows Save dialog", async () => {
    const destination = path.join(temp, "exported-session.jsonl");
    await page.getByRole("button", { name: "导出", exact: true }).click();
    await page
      .getByRole("button", { name: "选择保存位置", exact: true })
      .click();
    await new Promise<void>((resolve, reject) => {
      const helper = spawn("powershell.exe", [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        path.resolve("scripts/save-test-dialog.ps1"),
        "-Destination",
        destination,
      ]);
      let error = "";
      helper.stderr.on("data", (chunk) => {
        error += chunk;
      });
      helper.once("error", reject);
      helper.once("exit", (code) =>
        code === 0 ? resolve() : reject(new Error(error)),
      );
    });
    await expect
      .poll(async () => {
        try {
          return await readFile(destination, "utf8");
        } catch {
          return "";
        }
      })
      .toBe(original);
    await expect(page.getByRole("dialog")).toHaveCount(0);
  });
  test("bulk archives and restores without deleting session files", async () => {
    await page
      .getByRole("checkbox", { name: "选择当前显示的全部会话" })
      .check();
    await page.getByRole("button", { name: "批量归档", exact: true }).click();
    await page.getByRole("button", { name: "确认归档", exact: true }).click();
    await expect(page.locator(".session-card")).toHaveCount(0);
    await page.getByRole("button", { name: "已归档 2", exact: true }).click();
    await expect(page.locator(".session-card")).toHaveCount(2);
    await page
      .getByRole("checkbox", { name: "选择当前显示的全部会话" })
      .check();
    await page.getByRole("button", { name: "批量恢复", exact: true }).click();
    await page.getByRole("button", { name: "恢复会话", exact: true }).click();
    await page.getByRole("button", { name: "全部会话 2", exact: true }).click();
    await expect(page.locator(".session-card")).toHaveCount(2);
    expect(await readFile(file, "utf8")).toBe(original);
  });
  test("native file changes update the window without manual refresh", async () => {
    await appendFile(
      file,
      JSON.stringify({
        type: "message",
        id: "live-a3",
        parentId: "a2",
        timestamp: new Date().toISOString(),
        message: {
          role: "assistant",
          content: [{ type: "text", text: "实时文件监听测试通过" }],
        },
      }) + "\n",
    );
    await expect(
      page
        .locator(".conversation")
        .getByText("实时文件监听测试通过", { exact: true }),
    ).toBeVisible({ timeout: 20_000 });
  });
  test("native settings, launch confirmation and themes work", async () => {
    await page.getByRole("button", { name: "在终端继续", exact: true }).click();
    await expect(page.getByRole("dialog")).toContainText("PowerShell");
    await page.getByRole("checkbox", { name: /分叉后继续/ }).check();
    await expect(
      page.getByRole("button", { name: "分叉并打开终端" }),
    ).toBeEnabled();
    await page.getByRole("button", { name: "取消", exact: true }).click();
    await page.getByRole("button", { name: "设置", exact: true }).click();
    await expect(page.getByRole("dialog")).toContainText("PI 会话目录");
    const terminalSelect = page.getByLabel("默认终端", { exact: true });
    const config = await command<Bootstrap>("bootstrap");
    const preferred = config.terminalOptions.find(
      (terminal) => terminal.available,
    )!;
    await expect(terminalSelect).toHaveValue(preferred.id);
    await expect(terminalSelect.locator("option:checked")).toHaveText(
      preferred.label,
    );
    await expect(terminalSelect).not.toContainText("自动");
    await expect(terminalSelect.locator("option")).toHaveCount(
      config.terminalOptions.length,
    );
    for (const terminal of config.terminalOptions) {
      await terminalSelect.selectOption(terminal.id);
      await expect(terminalSelect).toBeEnabled();
      await expect(terminalSelect).toHaveValue(terminal.id);
      await expect(page.getByRole("dialog")).toContainText(
        `当前使用：${terminal.label}`,
      );
    }
    await terminalSelect.selectOption(preferred.id);
    await expect(terminalSelect).toBeEnabled();
    await expect(terminalSelect).toHaveValue(preferred.id);
    await expect(
      command("set_terminal", { preference: "cmd.exe & echo unsafe" }),
    ).rejects.toBeTruthy();
    expect((await command<Bootstrap>("bootstrap")).terminalPreference).toBe(
      preferred.id,
    );
    await page.screenshot({
      path: "test-results/desktop-terminal-settings.png",
    });
    await expect(
      page.getByRole("button", { name: "选择会话目录" }),
    ).toBeEnabled();
    await page.getByRole("button", { name: "关闭弹窗" }).click();
    await page
      .getByRole("button", { name: "切换深色模式", exact: true })
      .click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await page.screenshot({ path: "test-results/desktop-dark.png" });
    await page
      .getByRole("button", { name: "切换浅色模式", exact: true })
      .click();
    // Restore the unconfigured state to verify legacy/default preference behavior.
    await command("set_terminal", { preference: "auto" });
    await page.reload();
  });
  test("automatic terminal opens the preferred PowerShell with correct fork arguments and a real TTY", async () => {
    const config = await command<Bootstrap>("bootstrap");
    expect(config.terminalPreference).toBe("auto");
    await page.getByRole("button", { name: "在终端继续", exact: true }).click();
    await page.getByRole("checkbox", { name: /分叉后继续/ }).check();
    await page
      .getByRole("button", { name: "分叉并打开终端", exact: true })
      .click();
    const record = await terminalRecord();
    const hasPwsh = config.terminalOptions.some(
      (option) => option.id === "pwsh",
    );
    expect(record.majorVersion).toBe(hasPwsh ? 7 : 5);
    expect(path.basename(record.executable).toLowerCase()).toBe(
      hasPwsh ? "pwsh.exe" : "powershell.exe",
    );
    expect(record.arguments[0]).toBe("--fork");
    expect(record.arguments).toContain("--name");
    expect(record.arguments).toContain("桌面端重命名验证");
    expect(await realpath(record.cwd)).toBe(
      await realpath(path.join(temp, "workspace")),
    );
    expect(record.outputRedirected).toBe(false);
    expect(record.inputRedirected).toBe(false);
    await stopTerminal();
  });
  test("explicit Windows PowerShell selection starts new sessions in the chosen terminal", async () => {
    await chooseTerminal("powershell");
    await page.getByRole("button", { name: "新建会话", exact: true }).click();
    await expect(page.getByRole("dialog")).toContainText(
      "Windows PowerShell · pi",
    );
    const project = page.getByLabel("工作目录", { exact: true });
    const options = await project
      .locator("option")
      .evaluateAll((items) =>
        items.map((item) => (item as HTMLOptionElement).value),
      );
    await project.selectOption(options.find(Boolean)!);
    await page.getByRole("button", { name: "打开终端", exact: true }).click();
    const record = await terminalRecord();
    expect(record.majorVersion).toBe(5);
    expect(path.basename(record.executable).toLowerCase()).toBe(
      "powershell.exe",
    );
    expect(record.arguments).toEqual([]);
    expect(await realpath(record.cwd)).toBe(
      await realpath(path.join(temp, "workspace")),
    );
    expect(record.outputRedirected).toBe(false);
    expect(record.inputRedirected).toBe(false);
    await stopTerminal();
  });
  test("sessions larger than 256 MiB remain listed and readable without the size warning", async () => {
    test.setTimeout(60_000);
    const temporary = path.join(temp, "large-session.tmp");
    const large = path.join(root, "large.jsonl");
    const handle = await open(temporary, "w");
    try {
      await handle.writeFile(
        JSON.stringify({
          type: "session",
          version: 3,
          id: "large-e2e",
          cwd: path.join(temp, "workspace"),
        }) + "\n",
      );
      await handle.writeFile(
        JSON.stringify({
          type: "session_info",
          id: "large-name",
          parentId: null,
          name: "大文件读取验证",
        }) + "\n",
      );
      const image = "A".repeat(8 * 1024 * 1024);
      for (let i = 0; i < 33; i++) {
        await handle.writeFile(
          `{"type":"message","id":"large-${i}","parentId":"${i === 0 ? "large-name" : `large-${i - 1}`}","message":{"role":"user","content":[{"type":"image","mimeType":"image/png","data":"${image}"}]}}\n`,
        );
      }
      await handle.writeFile(
        JSON.stringify({
          type: "message",
          id: "large-tail",
          parentId: "large-32",
          message: { role: "assistant", content: "大文件末尾消息读取成功" },
        }) + "\n",
      );
    } finally {
      await handle.close();
    }
    await rename(temporary, large);
    try {
      const index = await command<SessionIndex>("refresh_sessions");
      expect(index.warnings).not.toContainEqual(
        expect.stringContaining("256 MiB"),
      );
      const session = index.sessions.find((item) => item.id === "large-e2e")!;
      expect(session).toBeDefined();
      expect(session.fileSize).toBeGreaterThan(256 * 1024 * 1024);
      await page
        .locator(".session-card")
        .filter({ hasText: "大文件读取验证" })
        .locator(".session-select")
        .click();
      await expect(
        page
          .locator(".conversation")
          .getByText("大文件末尾消息读取成功", { exact: true }),
      ).toBeVisible({ timeout: 20_000 });
      await expect(
        page.getByText("会话超过 256 MiB，请直接在 pi 中查看", {
          exact: false,
        }),
      ).toHaveCount(0);
      const detail = await command<SessionDetail>("session_detail", {
        key: session.key,
        branch: "active",
        limit: 2,
      });
      expect(detail.entries.map((entry) => entry.id)).toEqual([
        "large-32",
        "large-tail",
      ]);
      expect(detail.warning).toBeNull();
      expect(JSON.stringify(detail).length).toBeLessThan(4096);
    } finally {
      await rm(large, { force: true });
      await command("refresh_sessions");
    }
  });
  test("close keeps the app in the tray; restore, single instance and Quit use native events", async () => {
    test.setTimeout(120_000);
    const pid = app.pid;
    // Previous terminal tests can leave the desktop window minimized.
    await desktopWindow("TrayClick");
    await expectWindow({ visible: true, minimized: false });
    await desktopWindow("Close");
    await expectWindow({ visible: false });
    expect(app.exitCode).toBeNull();
    expect(app.pid).toBe(pid);
    expect((await command<Bootstrap>("bootstrap")).sessionRoot).toBe(root);

    // The filesystem watcher and WebView stay alive while the window is hidden.
    const backgroundFile = path.join(root, "tray-background.jsonl");
    await writeFile(
      backgroundFile,
      JSON.stringify({
        type: "session",
        version: 3,
        id: "tray-background",
        cwd: path.join(temp, "workspace"),
      }) + "\n",
    );
    try {
      await expect
        .poll(async () =>
          (await command<SessionIndex>("list_sessions")).sessions.some(
            (session) => session.id === "tray-background",
          ),
        )
        .toBe(true);
    } finally {
      await rm(backgroundFile, { force: true });
      await command("refresh_sessions");
    }

    await desktopWindow("TrayClick");
    await expectWindow({ visible: true, minimized: false });
    await desktopWindow("Minimize");
    await expectWindow({ minimized: true });
    await desktopWindow("Close");
    await expectWindow({ visible: false });
    const menu = await desktopWindow("TrayMenu", "显示主窗口");
    expect(menu.items).toEqual(["显示主窗口", "退出"]);
    await expectWindow({ visible: true, minimized: false });

    await desktopWindow("Close");
    await expectWindow({ visible: false });
    await execFileAsync(binary, [], { timeout: 15_000 });
    await expectWindow({ visible: true, minimized: false });
    expect(app.pid).toBe(pid);
    expect(app.exitCode).toBeNull();

    await desktopWindow("Maximize");
    await expectWindow({ maximized: true });
    await desktopWindow("Close");
    await expectWindow({ visible: false });
    const quitMenu = await desktopWindow("TrayMenu", "退出");
    expect(quitMenu.items).toEqual(["显示主窗口", "退出"]);
    await expect.poll(() => app.exitCode).toBe(0);
    await browser.close().catch(() => {});
    await startApp("桌面端重命名验证");
    await expectWindow({
      visible: true,
      minimized: false,
      maximized: true,
    });
  });
  test("metadata and terminal selection survive application restart", async () => {
    await stopApp();
    await startApp("桌面端重命名验证");
    const config = await command<Bootstrap>("bootstrap");
    expect(config.terminalPreference).toBe("powershell");
    expect(config.terminal).toBe("Windows PowerShell");
    await page.getByRole("button", { name: "设置", exact: true }).click();
    await expect(page.getByLabel("默认终端", { exact: true })).toHaveValue(
      "powershell",
    );
    await page.getByRole("button", { name: "关闭弹窗" }).click();
    await expect(
      page.getByRole("heading", { name: "桌面端重命名验证", level: 2 }),
    ).toBeVisible();
  });
});
