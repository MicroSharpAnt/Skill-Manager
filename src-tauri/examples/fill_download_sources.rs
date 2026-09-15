//! Apply independently verified source metadata from a JSON manifest.
//! Usage: fill_download_sources <manifest.json> [--apply]
use serde::Deserialize;
use sha2::{Digest, Sha256};
use skill_manager::global::{DownloadSource, Manager};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Assignment {
    source: PathBuf,
    download_source: DownloadSource,
    skill_sha256: String,
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let manifest = args.get(1).ok_or("manifest.json required")?;
    let assignments: Vec<Assignment> = serde_json::from_slice(
        &fs::read(manifest).map_err(|e| e.to_string())?,
    ).map_err(|e| e.to_string())?;
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME missing")?);
    let data = home.join("Library/Application Support/com.yangjie.skillmanager/global");
    let m = Manager::new(&home, &data)?;
    let inventory = m.inventory()?;
    let mut sources = BTreeMap::new();
    for a in assignments {
        let source = a.source.canonicalize().map_err(|e| e.to_string())?;
        let bytes = fs::read(source.join("SKILL.md")).map_err(|e| e.to_string())?;
        if format!("{:x}", Sha256::digest(&bytes)) != a.skill_sha256 {
            return Err(format!("Verified content changed: {}", source.display()));
        }
        let skill = inventory.skills.iter().find(|s| PathBuf::from(&s.source) == source)
            .ok_or_else(|| format!("Skill not found: {}", source.display()))?;
        if sources.insert(skill.id.clone(), a.download_source).is_some() {
            return Err("Duplicate assignment".into());
        }
    }
    let plan = m.prepare_download_sources(sources)?;
    println!("{}", serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?);
    if args.get(2).map(String::as_str) == Some("--apply") {
        m.apply_admin(&plan.token)?;
        eprintln!("Applied metadata only; undo token: {}", plan.token);
    }
    Ok(())
}
