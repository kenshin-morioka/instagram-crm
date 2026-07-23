mod api;
mod auth;
mod commands;
mod config;
mod db;
mod error;
mod models;
mod scheduler;
mod services;
mod state;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent};

use crate::auth::token_store;
use crate::db::Db;
use crate::state::AppState;

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("instagram-crm".into()),
                    }),
                ])
                .build(),
        )
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;

            let config = config::load_or_create(&config_dir)?;
            let db = Db::open(&data_dir.join("app.db"))?;

            // 前回クラッシュ時に送信中のまま残った行は結果不明として自動再送しない
            match db.mark_stale_processing_as_unknown() {
                Ok(n) if n > 0 => {
                    log::warn!("送信結果不明の返信が{}件あります (RUNBOOK参照)", n)
                }
                Ok(_) => {}
                Err(e) => log::error!("processing行の復旧に失敗: {}", e),
            }

            let token = token_store::load().unwrap_or_else(|e| {
                log::error!("保存済みトークンの読み込みに失敗: {}", e);
                None
            });

            app.manage(AppState::new(config, db, token));

            setup_tray(app.handle())?;
            scheduler::polling::spawn(app.handle().clone());
            log::info!("起動しました");
            Ok(())
        })
        .on_window_event(|window, event| {
            // ウィンドウを閉じても常駐を続け、トレイのQuitでのみ終了する
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_terms_accepted,
            commands::accept_terms,
            commands::get_settings,
            commands::save_reply_text,
            commands::save_polling_interval,
            commands::save_fetch_settings,
            commands::set_dry_run,
            commands::set_sending_paused,
            commands::connect_with_token,
        ]);

    match builder.build(tauri::generate_context!()) {
        Ok(app) => app.run(|_, _| {}),
        Err(e) => {
            eprintln!("アプリの起動に失敗しました: {}", e);
            std::process::exit(1);
        }
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "開く", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                log::info!("Quitが選択されたため終了します");
                app.exit(0);
            }
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    tray.build(app)?;
    Ok(())
}
