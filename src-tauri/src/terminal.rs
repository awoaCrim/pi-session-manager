use base64::Engine;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalPreference {
    #[default]
    Auto,
    Pwsh,
    Powershell,
    System,
}
impl TerminalPreference {
    pub fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Pwsh => "pwsh",
            Self::Powershell => "powershell",
            Self::System => "system",
        }
    }
    pub fn from_id(id: &str) -> Result<Self, String> {
        match id {
            "auto" => Ok(Self::Auto),
            "pwsh" => Ok(Self::Pwsh),
            "powershell" => Ok(Self::Powershell),
            "system" => Ok(Self::System),
            _ => Err("不支持的终端选项".into()),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "自动检测",
            Self::Pwsh => "PowerShell 7",
            Self::Powershell => "Windows PowerShell",
            Self::System => "系统终端",
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct TerminalOption {
    pub id: TerminalPreference,
    pub label: String,
    pub available: bool,
}
pub struct TerminalConfig {
    pub label: String,
    pub options: Vec<TerminalOption>,
}
#[derive(Debug)]
struct Terminal {
    preference: TerminalPreference,
    label: String,
    command: String,
}
// Do not search the working directory or relative PATH entries for a shell.
fn find_executable(name: &str, path: Option<&OsStr>, fallbacks: Vec<PathBuf>) -> Option<String> {
    let paths = path
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name));
    paths
        .chain(fallbacks)
        .find(|candidate| candidate.is_absolute() && candidate.is_file())
        .map(|candidate| candidate.to_string_lossy().into_owned())
}
fn windows_terminals(pwsh: Option<String>, powershell: Option<String>) -> Vec<Terminal> {
    [
        (TerminalPreference::Pwsh, pwsh),
        (TerminalPreference::Powershell, powershell),
    ]
    .into_iter()
    .filter_map(|(preference, command)| {
        command.map(|command| Terminal {
            preference,
            label: preference.label().into(),
            command,
        })
    })
    .collect()
}
fn detect() -> Vec<Terminal> {
    if cfg!(windows) {
        let path = std::env::var_os("PATH");
        let pwsh_paths = ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(|dir| {
                PathBuf::from(dir)
                    .join("PowerShell")
                    .join("7")
                    .join("pwsh.exe")
            })
            .collect();
        let system_powershell = std::env::var_os("SystemRoot")
            .map(|dir| {
                PathBuf::from(dir)
                    .join("System32")
                    .join("WindowsPowerShell")
                    .join("v1.0")
                    .join("powershell.exe")
            })
            .into_iter()
            .collect();
        return windows_terminals(
            find_executable("pwsh.exe", path.as_deref(), pwsh_paths),
            find_executable("powershell.exe", None, system_powershell)
                .or_else(|| find_executable("powershell.exe", path.as_deref(), Vec::new())),
        );
    }
    let (label, command) = if cfg!(target_os = "macos") {
        ("Terminal.app".into(), "osascript".into())
    } else {
        let command = std::env::var("PI_SESSION_MANAGER_TERMINAL")
            .unwrap_or_else(|_| "x-terminal-emulator".into());
        (command.clone(), command)
    };
    vec![Terminal {
        preference: TerminalPreference::System,
        label,
        command,
    }]
}
fn select(terminals: &[Terminal], preference: TerminalPreference) -> Result<&Terminal, String> {
    if preference == TerminalPreference::Auto {
        terminals
            .first()
            .ok_or_else(|| "未检测到可用终端，请安装 PowerShell 7 后重启应用。".into())
    } else {
        terminals
            .iter()
            .find(|terminal| terminal.preference == preference)
            .ok_or_else(|| {
                format!(
                    "未检测到 {}，请在设置中选择其他终端或改为自动。",
                    preference.label()
                )
            })
    }
}
pub fn validate(preference: TerminalPreference) -> Result<(), String> {
    if preference == TerminalPreference::Auto {
        return Ok(());
    }
    select(&detect(), preference).map(|_| ())
}
pub fn configuration(preference: TerminalPreference) -> TerminalConfig {
    let terminals = detect();
    let label = select(&terminals, preference)
        .map(|terminal| terminal.label.clone())
        .unwrap_or_else(|_| {
            if preference == TerminalPreference::Auto {
                "未检测到可用终端".into()
            } else {
                format!("{}（不可用）", preference.label())
            }
        });
    let mut options: Vec<_> = terminals
        .into_iter()
        .map(|terminal| TerminalOption {
            id: terminal.preference,
            label: terminal.label,
            available: true,
        })
        .collect();
    if preference != TerminalPreference::Auto
        && !options.iter().any(|option| option.id == preference)
    {
        options.push(TerminalOption {
            id: preference,
            label: label.clone(),
            available: false,
        });
    }
    TerminalConfig { label, options }
}

#[derive(Debug)]
pub struct Launch {
    pub cwd: String,
    pub path: Option<String>,
    pub name: Option<String>,
    pub fork: bool,
}
#[derive(Debug)]
pub struct Plan {
    pub command: String,
    pub args: Vec<String>,
}
fn powershell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}
pub fn plan(request: &Launch, platform: &str, executable: &str, terminal_command: &str) -> Plan {
    let mut args = vec![executable.to_owned()];
    if let Some(path) = &request.path {
        args.extend([
            if request.fork { "--fork" } else { "--session" }.into(),
            path.clone(),
        ]);
    }
    if let Some(name) = &request.name {
        args.extend(["--name".into(), name.clone()]);
    }
    if platform == "windows" {
        let script = format!(
            "$Host.UI.RawUI.WindowTitle = 'Pi Session'; Set-Location -LiteralPath {}; $piCommand = Get-Command {} -ErrorAction Stop; if ($piCommand.Source -match '\\.(cmd|bat)$') {{ throw 'Use the npm pi.ps1 shim or configure PI_SESSION_MANAGER_PI_BIN to a PowerShell script or executable.' }}; & {}",
            powershell_quote(&request.cwd),
            powershell_quote(executable),
            args.iter()
                .map(|s| powershell_quote(s))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        return Plan {
            command: terminal_command.into(),
            args: vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-NoExit".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-EncodedCommand".into(),
                base64::engine::general_purpose::STANDARD.encode(bytes),
            ],
        };
    }
    let command = format!(
        "cd -- {} && {}",
        shell_quote(&request.cwd),
        args.iter()
            .map(|s| shell_quote(s))
            .collect::<Vec<_>>()
            .join(" ")
    );
    if platform == "macos" {
        let apple_string = command
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        return Plan { command: terminal_command.into(), args: vec!["-e".into(), format!("tell application \"Terminal\"\nactivate\ndo script \"{apple_string}\"\nend tell")] };
    }
    Plan {
        command: terminal_command.into(),
        args: vec![
            "-e".into(),
            "sh".into(),
            "-lc".into(),
            format!("{command}; printf '\\nPi 已退出，按 Enter 关闭窗口'; read answer"),
        ],
    }
}
pub fn launch(request: Launch, preference: TerminalPreference) -> Result<String, String> {
    if !Path::new(&request.cwd).is_dir() {
        return Err("工作目录不存在，请先恢复目录或选择其他项目。".into());
    }
    if [&request.cwd]
        .into_iter()
        .chain(request.path.iter())
        .chain(request.name.iter())
        .any(|s| s.contains('\0'))
    {
        return Err("路径或名称包含无效字符".into());
    }
    let terminals = detect();
    let terminal = select(&terminals, preference)?;
    let executable = std::env::var("PI_SESSION_MANAGER_PI_BIN").unwrap_or_else(|_| "pi".into());
    let plan = plan(
        &request,
        std::env::consts::OS,
        &executable,
        &terminal.command,
    );
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A hidden bootstrap asks Windows to create an independent console. Passing NUL
        // stdio directly to the interactive process would make Pi think it has no TTY.
        // Only fixed switches and base64 cross this second shell boundary.
        let script = format!(
            "$ErrorActionPreference='Stop'; Start-Process -FilePath {} -ArgumentList @({})",
            powershell_quote(&plan.command),
            plan.args
                .iter()
                .map(|a| powershell_quote(a))
                .collect::<Vec<_>>()
                .join(",")
        );
        let status = Command::new(&plan.command)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ])
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("无法启动 {}：{e}", terminal.label))?;
        if !status.success() {
            return Err(format!(
                "Windows 拒绝启动 {}，请检查终端是否可用及系统权限。",
                terminal.label
            ));
        }
    }
    #[cfg(not(windows))]
    {
        let mut child = Command::new(plan.command)
            .args(plan.args)
            .current_dir(&request.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("无法启动系统终端：{e}"))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    Ok(format!(
        "已打开 {}。若终端提示找不到 pi，请检查 PATH 或 PI_SESSION_MANAGER_PI_BIN。",
        terminal.label
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_prefers_pwsh_and_falls_back_to_windows_powershell() {
        let terminals = windows_terminals(Some("pwsh.exe".into()), Some("powershell.exe".into()));
        assert_eq!(
            select(&terminals, TerminalPreference::Auto)
                .unwrap()
                .command,
            "pwsh.exe"
        );
        assert_eq!(
            select(&terminals, TerminalPreference::Powershell)
                .unwrap()
                .command,
            "powershell.exe"
        );
        let fallback = windows_terminals(None, Some("powershell.exe".into()));
        assert_eq!(
            select(&fallback, TerminalPreference::Auto).unwrap().command,
            "powershell.exe"
        );
        assert!(select(&fallback, TerminalPreference::Pwsh).is_err());
        assert!(select(&[], TerminalPreference::Auto).is_err());
    }
    #[test]
    fn discovery_searches_path_and_standard_install_without_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path_install = dir.path().join("portable");
        let standard_install = dir
            .path()
            .join("Program Files")
            .join("PowerShell")
            .join("7");
        std::fs::create_dir_all(&path_install).unwrap();
        std::fs::create_dir_all(&standard_install).unwrap();
        let standard = standard_install.join("pwsh.exe");
        std::fs::write(&standard, b"fixture").unwrap();
        let path = std::env::join_paths([PathBuf::from("relative"), path_install.clone()]).unwrap();
        assert_eq!(
            find_executable("pwsh.exe", Some(&path), vec![standard.clone()]),
            Some(standard.to_string_lossy().into())
        );
        let portable = path_install.join("pwsh.exe");
        std::fs::write(&portable, b"fixture").unwrap();
        assert_eq!(
            find_executable("pwsh.exe", Some(&path), vec![standard]),
            Some(portable.to_string_lossy().into())
        );
        assert!(
            find_executable("pwsh.exe", None, vec![PathBuf::from("relative/pwsh.exe")]).is_none()
        );
    }
    #[test]
    fn windows_arguments_remain_literals_in_both_powershells() {
        let req = Launch {
            cwd: "C:\\用户\\it's a folder".into(),
            path: Some("C:\\sessions\\a&b.jsonl".into()),
            name: Some("'; $(Write-Output unsafe); '".into()),
            fork: false,
        };
        for command in [
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
            "powershell.exe",
        ] {
            let p = plan(&req, "windows", "pi.ps1", command);
            assert_eq!(p.command, command);
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(p.args.last().unwrap())
                .unwrap();
            let script = String::from_utf16(
                &bytes
                    .chunks_exact(2)
                    .map(|v| u16::from_le_bytes([v[0], v[1]]))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            assert!(script.contains("'C:\\用户\\it''s a folder'"));
            assert!(script.contains("'--session' 'C:\\sessions\\a&b.jsonl'"));
            assert!(script.contains("''; $(Write-Output unsafe); ''"));
        }
    }
    #[test]
    fn posix_quotes_do_not_expand_shell_tokens() {
        assert_eq!(shell_quote("a'b$(evil)`x`"), "'a'\"'\"'b$(evil)`x`'");
        let p = plan(
            &Launch {
                cwd: "/tmp/a b".into(),
                path: Some("/tmp/s.jsonl".into()),
                name: None,
                fork: true,
            },
            "linux",
            "pi",
            "x-terminal-emulator",
        );
        assert!(p.args.last().unwrap().contains("'--fork' '/tmp/s.jsonl'"));
    }
    #[test]
    fn missing_directory_returns_error() {
        assert!(launch(
            Launch {
                cwd: "/this-directory-does-not-exist-pi-test".into(),
                path: None,
                name: None,
                fork: false
            },
            TerminalPreference::Auto
        )
        .is_err());
    }
}
