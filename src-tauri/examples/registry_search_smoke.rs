//! Read-only online check of the same search path used by the desktop app.
fn main() -> Result<(), String> {
    let result = skill_manager::registry::search("super", 0)?;
    if result.skills.is_empty() {
        return Err("Search returned no skills for super".into());
    }
    println!("PASS: super returned {} skills", result.skills.len());
    Ok(())
}
