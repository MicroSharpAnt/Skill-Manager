//! Repair only inventoried dangling Skill links, with original links kept for undo.
use super::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokenLink {
    pub path: String,
    pub name: String,
    pub old_target: String,
    pub target: Option<String>,
    pub problem: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokenLinks {
    pub target_root: String,
    pub links: Vec<BrokenLink>,
    pub warnings: Vec<String>,
}

pub(super) fn require_broken(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)
        .map_err(e)?
        .file_type()
        .is_symlink()
    {
        return Err(format!("已不是软链接，请重新扫描：{}", path.display()));
    }
    match fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(format!("软链接已恢复有效，不会覆盖：{}", path.display())),
        Err(error) => Err(format!("无法确认链接是否断开：{}：{error}", path.display())),
    }
}

impl Manager {
    pub fn broken_links(&self, target_root: &str) -> Result<BrokenLinks> {
        let _lock = self.lock()?;
        self.scan_broken_links(target_root)
    }

    fn scan_broken_links(&self, target_root: &str) -> Result<BrokenLinks> {
        let config = self.config()?;
        let root = if target_root.trim().is_empty() {
            let independent = self.home.join(".skill-manager/skills");
            if independent.is_dir() {
                independent
            } else {
                self.library()
            }
        } else if let Some(relative) = target_root.trim().strip_prefix("~/") {
            self.home.join(relative)
        } else {
            PathBuf::from(target_root.trim())
        };
        if !root.is_absolute() {
            return Err("请选择绝对路径的技能目录".into());
        }
        let mut warnings = vec![];
        let root_problem = if !root.is_dir() {
            Some("目标技能目录不存在或无法读取，请重新选择目录".to_string())
        } else {
            None
        };
        let mut seen = BTreeSet::new();
        let mut links = vec![];
        for location in self.skill_locations(&config) {
            if !exists(&location) {
                continue;
            }
            let dir = match location.canonicalize() {
                Ok(dir) => dir,
                Err(error) => {
                    warnings.push(format!("{}：{error}", location.display()));
                    continue;
                }
            };
            if !seen.insert(dir.clone()) {
                continue;
            }
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(error) => {
                    warnings.push(format!("{}：{error}", dir.display()));
                    continue;
                }
            };
            for entry in entries {
                let entry = entry.map_err(e)?;
                let path = entry.path();
                if !entry.file_type().map_err(e)?.is_symlink() {
                    continue;
                }
                match fs::metadata(&path) {
                    Ok(_) => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        warnings.push(format!("无法确认链接状态：{}：{error}", path.display()));
                        continue;
                    }
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let candidate = (|| -> Result<PathBuf> {
                    validate_name(&name)?;
                    if let Some(problem) = &root_problem {
                        return Err(problem.clone());
                    }
                    let target = root
                        .join(&name)
                        .canonicalize()
                        .map_err(|_| "指定目录下没有可读取的同名 Skill".to_string())?;
                    if !target.is_dir() || !target.join("SKILL.md").is_file() {
                        return Err("同名目录缺少 SKILL.md".into());
                    }
                    if path.starts_with(&target) || target.starts_with(&path) {
                        return Err("目标与断链路径重叠，不能建立链接".into());
                    }
                    duplicates::duplicate_digest(&target)?;
                    Ok(target)
                })();
                links.push(BrokenLink {
                    name,
                    path: path.to_string_lossy().into(),
                    old_target: fs::read_link(&path).map_err(e)?.to_string_lossy().into(),
                    target: candidate.as_ref().ok().map(|p| p.to_string_lossy().into()),
                    problem: candidate.err(),
                });
            }
        }
        links.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
        Ok(BrokenLinks {
            target_root: root.to_string_lossy().into(),
            links,
            warnings,
        })
    }

    pub fn prepare_link_repair(&self, target_root: &str, paths: Vec<String>) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        if paths.is_empty() {
            return Err("请先选择需要修复的软链接".into());
        }
        let report = self.scan_broken_links(target_root)?;
        let mut p = self.admin_plan("repair-links")?;
        let mut used = BTreeSet::new();
        for path in paths {
            if !used.insert(path.clone()) {
                return Err("同一软链接不能重复选择".into());
            }
            let item = report
                .links
                .iter()
                .find(|item| item.path == path)
                .ok_or("链接已变化或不在全局扫描目录内，请重新扫描")?;
            let target = Path::new(
                item.target
                    .as_ref()
                    .ok_or_else(|| item.problem.clone().unwrap_or_default())?,
            );
            let path = Path::new(&path);
            require_broken(path)?;
            p.broken_checks.push(path.into());
            p.duplicate_checks
                .insert(target.into(), duplicates::duplicate_digest(target)?);
            let backup = self
                .admin_dir(&p.token)
                .join(format!("broken-link-{}", used.len()));
            let stage = self
                .admin_dir(&p.token)
                .join(format!("repaired-link-{}", used.len()));
            create_link(target, &stage)?;
            self.checked_move(&mut p, path, &backup)?;
            self.checked_move(&mut p, &stage, path)?;
            // Existing managed sources retain their metadata. Update only client expectations
            // for this exact destination; ordinary discovered sources remain unmanaged.
            for (client, _) in CLIENTS {
                if self.destination(client, &item.name)? == path {
                    for record in p.after.records.values_mut().filter(|r| r.name == item.name) {
                        if Path::new(&record.source) == target {
                            record.expected.insert((*client).into());
                        } else {
                            record.expected.remove(*client);
                        }
                    }
                }
            }
            p.after
                .config
                .copies
                .remove(&path.to_string_lossy().to_string());
            p.summary.push(format!(
                "{}\n断链位置：{}\n原指向：{}\n新指向：{}\n原链接备份：{}",
                item.name,
                path.display(),
                item.old_target,
                target.display(),
                backup.display()
            ));
        }
        self.finish_plan(p)
    }
}
