//! Read-only source inspection: stage the migration in temporary app data, never apply it.
use skill_manager::global::Manager;
use std::path::PathBuf;
fn main() -> Result<(), String> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?);
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let m = Manager::new(&home, temp.path())?;
    let p = m.prepare_cc(
        home.join(".cc-switch").to_string_lossy().into(),
        "independent".into(),
    )?;
    println!(
        "records {} -> {}; {} groups; {} repositories; {} file moves; {} warnings",
        p.before.records.len(),
        p.after.records.len(),
        p.after.config.groups.len(),
        p.after.config.repos.len(),
        p.moves.len(),
        p.warnings.len()
    );
    for warning in p.warnings {
        println!("{warning}");
    }
    println!("Preview only: no source files, links or cc-switch database were modified.");
    Ok(())
}
