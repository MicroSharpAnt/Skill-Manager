//! Explicit online smoke test; all installed files and metadata live in a temporary home.
use skill_manager::{global::Manager, registry};
use std::fs;
fn main() -> Result<(), String> {
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let home = scratch.path().join("home");
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let manager = Manager::new(&home, &scratch.path().join("data"))?;
    let results = registry::search("find-skills", 0)?;
    assert!(!results.skills.is_empty());
    println!("Search: {} results", results.skills.len());
    let discovery = manager.discover("vercel-labs/skills", "HEAD")?;
    let candidate = discovery
        .candidates
        .iter()
        .find(|c| c.path.ends_with("find-skills"))
        .ok_or("find-skills candidate missing")?;
    println!(
        "Discovery: {} candidates; exact path {}",
        discovery.candidates.len(),
        candidate.path
    );
    let plan = manager.prepare_remote(&discovery.token, &candidate.path, None)?;
    manager.apply(&plan.token, false)?;
    manager.toggle(&plan.record.id, vec!["Codex".into(), "Claude".into()], true)?;
    assert_eq!(manager.check_updates()?[0].status, "latest");
    let skill = home
        .join(".codex/skills")
        .join(&plan.record.name)
        .join("SKILL.md");
    let original = fs::read_to_string(&skill).map_err(|e| e.to_string())?;
    fs::write(&skill, format!("{original}\nSmoke-test local edit\n")).map_err(|e| e.to_string())?;
    assert_eq!(manager.check_updates()?[0].status, "local");
    let update = manager.prepare_update(&plan.record.id)?;
    assert!(manager.apply(&update.token, false).is_err());
    manager.apply(&update.token, true)?;
    assert_eq!(fs::read_to_string(&skill).unwrap(), original);
    let backups = manager.backups(&plan.record.id)?;
    let restore = manager.prepare_restore(&plan.record.id, &backups[0].token)?;
    manager.apply(&restore.token, true)?;
    assert!(fs::read_to_string(&skill)
        .unwrap()
        .contains("Smoke-test local edit"));
    manager.toggle(
        &plan.record.id,
        vec!["Codex".into(), "Claude".into()],
        false,
    )?;
    assert!(!skill.exists());
    assert!(std::path::Path::new(&plan.record.source)
        .join("SKILL.md")
        .exists());
    println!("PASS: real registry / GitHub archive / isolated install / links / updates / backup restore");
    Ok(())
}
