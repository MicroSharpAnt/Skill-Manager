use crate::{
    engine::{Change, Result, Snapshot},
    store::{Collection, Project, Store},
};
use std::{path::PathBuf, sync::Mutex};
use tauri::{Manager, State};
struct AppState {
    store: Mutex<Store>,
}
fn with_store<T>(s: State<AppState>, f: impl FnOnce(&Store) -> Result<T>) -> Result<T> {
    let store = s.store.lock().map_err(|_| "管理状态异常，请重启应用")?;
    f(&store)
}
#[tauri::command]
fn projects(s: State<AppState>) -> Result<Vec<Project>> {
    with_store(s, |s| s.projects())
}
#[tauri::command]
fn add_project(s: State<AppState>, path: String) -> Result<String> {
    with_store(s, |s| s.add(&path))
}
#[tauri::command]
fn snapshot(s: State<AppState>, root: String) -> Result<Snapshot> {
    with_store(s, |s| s.ensure_project(&root)?.scan())
}
#[tauri::command]
fn content(s: State<AppState>, root: String, path: String) -> Result<String> {
    with_store(s, |s| s.ensure_project(&root)?.content(&path))
}
#[tauri::command]
fn install_hook(s: State<AppState>, root: String, chain_existing: bool) -> Result<()> {
    with_store(s, |s| s.ensure_project(&root)?.install_hook(chain_existing))
}
#[tauri::command]
async fn apply(s: State<'_, AppState>, root: String, changes: Vec<Change>) -> Result<()> {
    let repo = with_store(s, |s| s.ensure_project(&root))?;
    tauri::async_runtime::spawn_blocking(move || repo.apply(changes))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
fn recover(s: State<AppState>, root: String) -> Result<()> {
    with_store(s, |s| s.ensure_project(&root)?.recover())
}
#[tauri::command]
fn collections(s: State<AppState>, root: String) -> Result<Vec<Collection>> {
    with_store(s, |s| s.collections(&root))
}
#[tauri::command]
fn save_collection(
    s: State<AppState>,
    root: String,
    name: String,
    kind: String,
    paths: Vec<String>,
) -> Result<()> {
    with_store(s, |s| s.save(&root, &name, &kind, paths))
}
#[tauri::command]
fn update_project_group(
    s: State<AppState>,
    root: String,
    id: i64,
    name: String,
    color: String,
) -> Result<()> {
    with_store(s, |s| s.update_group(&root, id, &name, &color))
}
#[tauri::command]
fn move_project_group(
    s: State<AppState>,
    root: String,
    paths: Vec<String>,
    destination: Option<i64>,
) -> Result<()> {
    with_store(s, |s| s.move_group(&root, paths, destination))
}
#[tauri::command]
fn reorder_project_groups(s: State<AppState>, root: String, ids: Vec<i64>) -> Result<()> {
    with_store(s, |s| s.reorder_groups(&root, ids))
}
#[tauri::command]
fn presets(s: State<AppState>, scope: String) -> Result<Vec<crate::store::Preset>> {
    with_store(s, |s| s.presets(&scope))
}
#[tauri::command]
fn save_preset(s: State<AppState>, scope: String, preset: crate::store::Preset) -> Result<()> {
    with_store(s, |s| s.save_preset(&scope, preset))
}
#[tauri::command]
fn delete_preset(s: State<AppState>, scope: String, id: String) -> Result<()> {
    with_store(s, |s| s.delete_preset(&scope, &id))
}
#[tauri::command]
fn delete_collection(s: State<AppState>, id: i64) -> Result<()> {
    with_store(s, |s| s.delete_collection(id))
}
fn global_manager(app: &tauri::AppHandle) -> Result<crate::global::Manager> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("找不到用户目录")?);
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("global");
    crate::global::Manager::new(&home, &data)
}
fn llm_dir(app: &tauri::AppHandle) -> Result<PathBuf> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}
#[tauri::command]
fn llm_config(app: tauri::AppHandle) -> Result<crate::llm::ConfigView> {
    crate::llm::config(&llm_dir(&app)?)
}
#[tauri::command]
fn llm_save_config(
    app: tauri::AppHandle,
    input: crate::llm::ConfigInput,
) -> Result<crate::llm::ConfigView> {
    crate::llm::save_config(&llm_dir(&app)?, input)
}
#[tauri::command]
async fn llm_models(app: tauri::AppHandle, input: crate::llm::ConfigInput) -> Result<Vec<String>> {
    let dir = llm_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || crate::llm::models(&dir, input))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn llm_generate(
    app: tauri::AppHandle,
    text: String,
    mode: String,
    language: String,
    on_event: tauri::ipc::Channel<crate::llm::GenerationProgress>,
) -> Result<String> {
    let dir = llm_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::llm::generate_streamed(&dir, &text, &mode, &language, &mut |event| {
            on_event
                .send(event)
                .map_err(|_| "已停止接收生成结果".into())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn llm_translate_segments(
    app: tauri::AppHandle,
    segments: Vec<crate::llm::TranslationSegment>,
    language: String,
) -> Result<Vec<crate::llm::TranslationSegment>> {
    let dir = llm_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::llm::translate_segments(&dir, segments, &language)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Deserialize)]
#[serde(tag = "scope", rename_all = "camelCase")]
enum LlmTarget {
    Project { root: String, path: String },
    Global { id: String },
}
#[tauri::command]
async fn llm_replace(
    app: tauri::AppHandle,
    s: State<'_, AppState>,
    target: LlmTarget,
    expected: String,
    replacement: String,
) -> Result<crate::llm::Replacement> {
    let backups = llm_dir(&app)?.join("translation-backups");
    match target {
        LlmTarget::Project { root, path } => {
            let repo = with_store(s, |s| s.ensure_project(&root))?;
            tauri::async_runtime::spawn_blocking(move || {
                repo.replace_translation(&path, &expected, &replacement, &backups)
            })
            .await
            .map_err(|e| e.to_string())?
        }
        LlmTarget::Global { id } => {
            global_job(app, move |m| {
                m.replace_translation(&id, &expected, &replacement, &backups)
            })
            .await
        }
    }
}
async fn global_job<T: Send + 'static>(
    app: tauri::AppHandle,
    job: impl FnOnce(crate::global::Manager) -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || job(global_manager(&app)?))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn global_inventory(app: tauri::AppHandle) -> Result<crate::global::Inventory> {
    global_job(app, |m| m.inventory()).await
}
#[tauri::command]
async fn codex_skill_usage(app: tauri::AppHandle, days: u32) -> Result<crate::usage::Report> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let codex = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    tauri::async_runtime::spawn_blocking(move || crate::usage::scan(&home, &codex, days))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn global_toggle(
    app: tauri::AppHandle,
    id: String,
    clients: Vec<String>,
    enable: bool,
) -> Result<()> {
    global_job(app, move |m| m.toggle(&id, clients, enable)).await
}
#[tauri::command]
async fn global_details(app: tauri::AppHandle, id: String) -> Result<String> {
    global_job(app, move |m| m.details(&id)).await
}
#[tauri::command]
async fn global_search(query: String, offset: usize) -> Result<crate::registry::SearchResult> {
    tauri::async_runtime::spawn_blocking(move || crate::registry::search(&query, offset))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn global_market_install_status(
    app: tauri::AppHandle,
    skills: Vec<crate::registry::MarketSkill>,
) -> Result<Vec<crate::global::install_status::InstallStatus>> {
    global_job(app, move |m| m.market_install_status(&skills)).await
}
#[tauri::command]
async fn global_discovery_install_status(
    app: tauri::AppHandle,
    discovery: String,
) -> Result<Vec<crate::global::install_status::InstallStatus>> {
    global_job(app, move |m| m.discovery_install_status(&discovery)).await
}
#[tauri::command]
async fn global_discover(
    app: tauri::AppHandle,
    repo: String,
    reference: String,
) -> Result<crate::global::Discovery> {
    global_job(app, move |m| m.discover(&repo, &reference)).await
}
#[tauri::command]
async fn global_discovery_details(
    app: tauri::AppHandle,
    discovery: String,
    path: String,
) -> Result<crate::global::DiscoveryDetails> {
    global_job(app, move |m| m.discovery_details(&discovery, &path)).await
}
#[tauri::command]
async fn global_prepare_remote(
    app: tauri::AppHandle,
    discovery: String,
    path: String,
    existing: Option<String>,
) -> Result<crate::global::Plan> {
    global_job(app, move |m| {
        m.prepare_remote(&discovery, &path, existing.as_deref())
    })
    .await
}
#[tauri::command]
async fn global_prepare_local(app: tauri::AppHandle, path: String) -> Result<crate::global::Plan> {
    global_job(app, move |m| m.prepare_local(std::path::Path::new(&path))).await
}
#[tauri::command]
async fn global_prepare_update(app: tauri::AppHandle, id: String) -> Result<crate::global::Plan> {
    global_job(app, move |m| m.prepare_update(&id)).await
}
#[tauri::command]
async fn global_apply(
    app: tauri::AppHandle,
    token: String,
    allow_local_changes: bool,
) -> Result<()> {
    global_job(app, move |m| m.apply(&token, allow_local_changes)).await
}
#[tauri::command]
async fn global_check_updates(app: tauri::AppHandle) -> Result<Vec<crate::global::Update>> {
    global_job(app, |m| m.check_updates()).await
}
#[tauri::command]
async fn global_backups(app: tauri::AppHandle, id: String) -> Result<Vec<crate::global::Backup>> {
    global_job(app, move |m| m.backups(&id)).await
}
#[tauri::command]
async fn global_prepare_restore(
    app: tauri::AppHandle,
    id: String,
    token: String,
) -> Result<crate::global::Plan> {
    global_job(app, move |m| m.prepare_restore(&id, &token)).await
}
#[tauri::command]
async fn global_recover(app: tauri::AppHandle) -> Result<()> {
    global_job(app, |m| m.recover()).await
}
#[tauri::command]
async fn global_admin(
    app: tauri::AppHandle,
    action: String,
    args: serde_json::Value,
) -> Result<serde_json::Value> {
    global_job(app, move |m| m.admin_command(&action, args)).await
}
#[tauri::command]
async fn open_skill_source(origin: crate::registry::Origin) -> Result<()> {
    let url = crate::registry::source_page_url(&origin)?;
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        let status = std::process::Command::new("open").arg(&url).status();
        #[cfg(target_os = "windows")]
        let status = std::process::Command::new("explorer.exe")
            .arg(&url)
            .status();
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let status = std::process::Command::new("xdg-open").arg(&url).status();
        if status.map_err(|e| e.to_string())?.success() {
            Ok(())
        } else {
            Err("无法打开来源网页".into())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Store::open(&dir.join("manager.sqlite")).map_err(std::io::Error::other)?;
            app.manage(AppState {
                store: Mutex::new(store),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            llm_config,
            llm_save_config,
            llm_models,
            llm_generate,
            llm_translate_segments,
            llm_replace,
            projects,
            add_project,
            snapshot,
            content,
            install_hook,
            apply,
            recover,
            collections,
            save_collection,
            presets,
            save_preset,
            delete_preset,
            update_project_group,
            move_project_group,
            reorder_project_groups,
            delete_collection,
            global_inventory,
            codex_skill_usage,
            global_toggle,
            global_details,
            global_search,
            global_market_install_status,
            global_discovery_install_status,
            global_discover,
            global_discovery_details,
            global_prepare_remote,
            global_prepare_local,
            global_prepare_update,
            global_apply,
            global_check_updates,
            global_backups,
            global_prepare_restore,
            global_recover,
            global_admin,
            open_skill_source
        ])
        .run(tauri::generate_context!())
        .expect("启动 Skill Manager 失败");
}
