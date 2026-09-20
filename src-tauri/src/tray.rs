use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, RunEvent, Window, WindowEvent,
};
use tauri_plugin_window_state::StateFlags;

const MAIN_WINDOW: &str = "main";
const SHOW: &str = "tray-show";
const QUIT: &str = "tray-quit";

pub fn window_state_flags() -> StateFlags {
    // A tray exit must not make the next launch start with an invisible window.
    StateFlags::all() & !StateFlags::VISIBLE
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        #[cfg(target_os = "windows")]
        if let Err(error) = window.set_skip_taskbar(false) {
            eprintln!("无法恢复任务栏图标: {error}");
        }
        let result = window
            .show()
            .and_then(|_| window.unminimize())
            .and_then(|_| window.set_focus());
        if let Err(error) = result {
            eprintln!("无法显示 Pi Sessions 主窗口: {error}");
        }
    }
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        if let Err(error) = window.hide() {
            eprintln!("无法隐藏 Pi Sessions 主窗口: {error}");
            return;
        }
        #[cfg(target_os = "windows")]
        if let Err(error) = window.set_skip_taskbar(true) {
            eprintln!("无法隐藏任务栏图标: {error}");
        }
    }
}

pub fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        RunEvent::Ready => show_main_window(app),
        // Runtime-generated requests must not terminate the tray. The Quit menu
        // calls app.exit(0), whose Some(0) code follows Tauri's normal cleanup.
        RunEvent::ExitRequested {
            api, code: None, ..
        } => api.prevent_exit(),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main_window(app),
        _ => {}
    }
}

pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, SHOW, "显示主窗口", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &separator, &quit])?;
    let icon = app
        .default_window_icon()
        .ok_or_else(|| std::io::Error::other("无法加载 Pi Sessions 托盘图标"))?;
    TrayIconBuilder::with_id("pi-sessions")
        .icon(icon.clone())
        .tooltip("Pi Sessions")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            SHOW => show_main_window(app),
            QUIT => app.exit(0),
            _ => {}
        })
        .build(app)?;
    #[cfg(target_os = "macos")]
    setup_macos_menu(app)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn setup_macos_menu(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::Submenu;
    // Keep this a menu-bar app: a regular Dock icon exposes a native Quit action
    // that can bypass Tauri's ExitRequested callback on macOS.
    app.set_activation_policy(tauri::ActivationPolicy::Accessory)?;
    // Keep native editing shortcuts without the default Cmd+Q / Quit action;
    // deliberate application exit is offered by the tray menu only.
    let application = Submenu::with_items(
        app,
        "Pi Sessions",
        true,
        &[
            &PredefinedMenuItem::about(app, None, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    app.set_menu(Menu::with_items(app, &[&application, &edit, &window])?)?;
    Ok(())
}
