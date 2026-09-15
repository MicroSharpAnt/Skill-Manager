//! Read-only diagnostic for the same report shown in the desktop app.
fn main() -> Result<(), String> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or("HOME 未设置")?;
    let codex = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let days = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "30".into())
        .parse::<u32>()
        .map_err(|e| e.to_string())?;
    let report = skill_manager::usage::scan(&home, &codex, days)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}
