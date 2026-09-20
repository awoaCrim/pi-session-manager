<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="Pi Sessions icon" width="96">
</p>

[English](README.md) | [简体中文](README.zh-CN.md)

# Pi Sessions

<p align="center">Desktop manager for local Pi sessions. Search, favorite, archive, export, and continue local JSONL sessions in a terminal.</p>

## Features

| Goal            | What it supports                                                                           |
| --------------- | ------------------------------------------------------------------------------------------ |
| Find sessions   | Search by name, project, model, ID, and message snippets, then group by working directory. |
| Manage sessions | Favorite, archive, restore, and rename individual sessions or batches.                     |
| Continue work   | Resume with pi's `--session`, or fork with `--fork` and continue.                          |
| Inspect records | View Markdown, code, thinking, tool calls, execution results, and branch records.          |
| Stay in sync    | Watch local JSONL changes and update the list and current details automatically.           |
| Export records  | Use the system save dialog to export a single JSONL file or a multi-session JSON package.  |

The manager stores only names, favorite status, and archive status in its own SQLite database. It does not rewrite the original JSONL files for these operations. When resuming a session, the name is passed through pi's command-line arguments.

Closing the main window keeps Pi Sessions running in the system tray, including session updates. Left-click the tray icon, choose “显示主窗口” (Show main window) from its right-click menu, or launch Pi Sessions again to restore the window. To exit the application, right-click the tray icon and choose “退出” (Quit). macOS uses a menu-bar icon without a Dock icon. Linux desktops need an AppIndicator-compatible tray; use its menu to restore the window, since tray click events are not supported on Linux.

## UI preview

<p align="center">
  <img src="docs/ui-light.png" alt="Pi Sessions light interface" width="49%">
  <img src="docs/ui-dark.png" alt="Pi Sessions dark interface" width="49%">
</p>

## Install and quick start

### Use a packaged release

Download the package for your platform from [GitHub Releases](https://github.com/awoaCrim/pi-session-manager/releases). Windows requires Microsoft Edge WebView2 Runtime. When Pi Sessions starts, it reads pi's default session directory.

The `v0.1.1` release provides a Windows x64 installer and portable executable, including the close-to-tray feature. Windows native desktop tests have passed. No macOS/Linux packages were built for this version; their previous `v0.1.0` DMG and DEB/AppImage packages remain available and do not include the tray update. Native macOS/Linux interaction tests are still pending.

### Run from source

You need Node.js 22.12 or later and Rust stable. Windows also requires C++ build tools and WebView2. Linux requires Tauri's WebKitGTK system dependencies. macOS requires Xcode Command Line Tools.

```bash
npm install
```

```bash
npm run dev
```

Start with synthetic data. Demo mode does not read real sessions or start a real terminal:

```bash
npm run demo
```

## Configuration

The application reads pi's session directory by default. You can select a custom directory in Settings. The selection is stored in the manager's own SQLite database.

Environment variable precedence:

1. `PI_CODING_AGENT_SESSION_DIR`
2. The session directory derived from `PI_CODING_AGENT_DIR`
3. pi's default user directory

On Windows, PowerShell 7 is preferred by default, with Windows PowerShell as the fallback. You can select a detected terminal in Settings > Default terminal. Continue after forking uses pi's `--fork` to avoid two processes writing to the same session.

On Linux, set `PI_SESSION_MANAGER_TERMINAL` to choose the terminal program. The default is `x-terminal-emulator`, and the terminal must support the `-e` argument. `PI_SESSION_MANAGER_PI_BIN` can specify the pi command name or a full path.

Export uses the system save dialog. Exported files may contain source code, terminal output, and credentials, so store them carefully. Session files are scanned in chunks and are not rejected just because the complete file is larger than 256 MiB. Individual records, text content, and single-detail previews still have limits to avoid loading an entire session into memory at once.

| Environment variable          | Purpose                                                                             |
| ----------------------------- | ----------------------------------------------------------------------------------- |
| `PI_CODING_AGENT_SESSION_DIR` | pi session root directory.                                                          |
| `PI_CODING_AGENT_DIR`         | pi configuration directory, used to derive the default session directory.           |
| `PI_SESSION_MANAGER_DATA_DIR` | Manager SQLite data directory.                                                      |
| `PI_SESSION_MANAGER_PI_BIN`   | pi command name or full executable path. Defaults to `pi`.                          |
| `PI_SESSION_MANAGER_TERMINAL` | Linux terminal program. Defaults to `x-terminal-emulator`, which must support `-e`. |

## Development and verification

Build the renderer:

```bash
npm run build:renderer
```

Run Rust unit tests:

```bash
npm test
```

Check TypeScript:

```bash
npm run typecheck
```

Check formatting:

```bash
npm run format:check
```

Build the Windows application and NSIS installer:

```bash
npm run build
```

Build the macOS DMG:

```bash
npm run build:macos
```

Build the Linux DEB and AppImage packages:

```bash
npm run build:linux
```

Build the native Windows test application with a separate application identifier, so it does not share the installed app's single-instance lock or window state:

```bash
npm run build:e2e
```

Run native Windows desktop tests:

```bash
npm run test:e2e
```

Desktop tests require Windows WebView2. They use a temporary session directory and a test pi script, and do not modify real pi sessions. Tray tests send native Windows close and tray notifications, dispatch commands resolved from the real tray menu, and verify background updates, window restoration, and graceful exit.

## License

Licensed under the [MIT License](LICENSE).

## Thanks

Thanks to [Linux.do](https://linux.do/).
