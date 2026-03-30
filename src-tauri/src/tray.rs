use tauri::{
    image::Image,
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

use crate::{
    recorder::{start_recording_inner, stop_recording_inner},
    state::AppState,
};

const TRAY_ID: &str = "main-tray";
const MENU_SHOW: &str = "show";
const MENU_START_RECORDING: &str = "start_recording";
const MENU_STOP_RECORDING: &str = "stop_recording";
const MENU_QUIT: &str = "quit";

const TRAY_TOOLTIP_IDLE: &str = "Kiroku - 待機中";
const TRAY_TOOLTIP_RECORDING: &str = "Kiroku - 記録中";

const IDLE_TRAY_ICON: &[u8] = include_bytes!("../icons/tray-idle.png");
const RECORDING_TRAY_ICON: &[u8] = include_bytes!("../icons/tray-recording.png");

pub fn setup_tray(app: &AppHandle) -> Result<(), tauri::Error> {
    let menu = MenuBuilder::new(app)
        .text(MENU_SHOW, "ウィンドウを表示")
        .separator()
        .text(MENU_START_RECORDING, "記録開始")
        .text(MENU_STOP_RECORDING, "記録停止")
        .separator()
        .text(MENU_QUIT, "終了")
        .build()?;

    let idle_icon = tray_icon_for_recording(false)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(idle_icon)
        .menu(&menu)
        .tooltip(TRAY_TOOLTIP_IDLE)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().0.as_str() {
            MENU_SHOW => {
                let _ = show_main_window(app);
            }
            MENU_START_RECORDING => {
                if let Some(state) = app.try_state::<AppState>() {
                    let app_handle = app.clone();
                    let state = state.inner().clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = start_recording_inner(app_handle, state).await;
                    });
                }
            }
            MENU_STOP_RECORDING => {
                if let Some(state) = app.try_state::<AppState>() {
                    let app_handle = app.clone();
                    let state = state.inner().clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = stop_recording_inner(app_handle, state).await;
                    });
                }
            }
            MENU_QUIT => {
                shutdown_app(app.clone());
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::DoubleClick { .. } => {
                let _ = show_main_window(&tray.app_handle());
            }
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => {
                let _ = show_main_window(&tray.app_handle());
            }
            _ => {}
        })
        .build(app)?;

    update_recording_tray_state(app, false)?;
    Ok(())
}

pub fn handle_close_requested(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

pub fn show_main_window(app: &AppHandle) -> Result<(), tauri::Error> {
    if let Some(window) = app.get_webview_window("main") {
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
    }
    Ok(())
}

pub fn update_recording_tray_state(
    app: &AppHandle,
    is_recording: bool,
) -> Result<(), tauri::Error> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_icon(Some(tray_icon_for_recording(is_recording)?))?;
        tray.set_tooltip(Some(if is_recording {
            TRAY_TOOLTIP_RECORDING
        } else {
            TRAY_TOOLTIP_IDLE
        }))?;
    }

    Ok(())
}

pub fn shutdown_app(app: AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let app_handle = app.clone();
        let state = state.inner().clone();
        tauri::async_runtime::spawn(async move {
            let _ = stop_recording_inner(app_handle.clone(), state.clone()).await;
            state.shutdown_vlm_server().await;
            app_handle.exit(0);
        });
    } else {
        app.exit(0);
    }
}

fn tray_icon_for_recording(is_recording: bool) -> Result<Image<'static>, tauri::Error> {
    let bytes = if is_recording {
        RECORDING_TRAY_ICON
    } else {
        IDLE_TRAY_ICON
    };

    Image::from_bytes(bytes)
}
