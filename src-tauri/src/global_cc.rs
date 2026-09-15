//! Read only the Skill tables and Skill-specific settings. Never open the source database writable.
use super::*;
use rusqlite::{types::ValueRef, OpenFlags};
#[derive(Clone, Serialize, Deserialize)]
struct CcSnapshot {
    rows: Vec<serde_json::Value>,
    groups: Vec<serde_json::Value>,
    repos: Vec<serde_json::Value>,
    settings: serde_json::Value,
}
fn rows(db: &Connection, table: &str, allowed: &[&str]) -> Result<Vec<serde_json::Value>> {
    let exists: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |r| r.get(0),
        )
        .map_err(e)?;
    if exists == 0 {
        return Ok(vec![]);
    }
    let mut stmt = db
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(e)?;
    let columns = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(e)?
        .collect::<std::result::Result<BTreeSet<_>, _>>()
        .map_err(e)?;
    let selected: Vec<_> = allowed
        .iter()
        .filter(|c| columns.contains(**c))
        .copied()
        .collect();
    if selected.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt = db
        .prepare(&format!(
            "SELECT {} FROM {table} ORDER BY 1",
            selected.join(",")
        ))
        .map_err(e)?;
    let mut cursor = stmt.query([]).map_err(e)?;
    let mut result = vec![];
    while let Some(row) = cursor.next().map_err(e)? {
        let mut obj = serde_json::Map::new();
        for (i, key) in selected.iter().enumerate() {
            let value = match row.get_ref(i).map_err(e)? {
                ValueRef::Null => serde_json::Value::Null,
                ValueRef::Integer(x) => x.into(),
                ValueRef::Real(x) => x.into(),
                ValueRef::Text(x) => String::from_utf8_lossy(x).to_string().into(),
                _ => return Err("Skill 元数据含意外二进制字段".into()),
            };
            obj.insert(key.to_string(), value);
        }
        result.push(obj.into());
    }
    Ok(result)
}
fn snapshot(dir: &Path) -> Result<CcSnapshot> {
    let db = Connection::open_with_flags(
        dir.join("cc-switch.db"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(e)?;
    db.execute_batch("PRAGMA query_only=ON; BEGIN;")
        .map_err(e)?;
    let rows = rows(
        &db,
        "skills",
        &[
            "id",
            "name",
            "description",
            "directory",
            "repo_owner",
            "repo_name",
            "repo_branch",
            "readme_url",
            "group_id",
            "enabled_claude",
            "enabled_codex",
            "enabled_opencode",
            "installed_at",
            "content_hash",
            "updated_at",
        ],
    )?;
    let groups = rows_table(&db, "skill_groups", &["id", "name", "color", "created_at"])?;
    let repos = rows_table(&db, "skill_repos", &["owner", "name", "branch", "enabled"])?;
    let mut settings = serde_json::Map::new();
    if dir.join("settings.json").is_file() {
        let all: serde_json::Value = read(&dir.join("settings.json"))?;
        for key in [
            "skillStorageLocation",
            "skillSyncMethod",
            "claudeConfigDir",
            "codexConfigDir",
            "opencodeConfigDir",
            "openclawConfigDir",
        ] {
            if let Some(v) = all.get(key) {
                settings.insert(key.into(), v.clone());
            }
        }
    }
    Ok(CcSnapshot {
        rows,
        groups,
        repos,
        settings: settings.into(),
    })
}
fn rows_table(db: &Connection, t: &str, a: &[&str]) -> Result<Vec<serde_json::Value>> {
    rows(db, t, a)
}
fn string(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").into()
}
fn enabled(v: &serde_json::Value, k: &str) -> bool {
    v.get(k)
        .is_some_and(|x| x.as_bool().unwrap_or_else(|| x.as_i64().unwrap_or(0) != 0))
}
fn origin(v: &serde_json::Value) -> Option<Origin> {
    let repo = format!("{}/{}", string(v, "repo_owner"), string(v, "repo_name"));
    let mut reference = string(v, "repo_branch");
    if reference.is_empty() {
        reference = "main".into()
    }
    registry::validate_repo(&repo, &reference).ok()?;
    let url = string(v, "readme_url");
    let decoded = percent_encoding::percent_decode_str(&url)
        .decode_utf8()
        .ok()?;
    let prefix = format!("https://github.com/{repo}/blob/{reference}/");
    let prefix_tree = format!("https://github.com/{repo}/tree/{reference}/");
    let path = decoded
        .strip_prefix(&prefix)
        .or_else(|| decoded.strip_prefix(&prefix_tree))?;
    let path = path.strip_suffix("/SKILL.md").unwrap_or(path);
    let path = if path == "SKILL.md" { "." } else { path };
    if path != "." {
        registry::relative(path).ok()?;
    }
    Some(Origin {
        repo,
        reference,
        path: path.into(),
    })
}
impl Manager {
    pub(super) fn cc_fingerprint(&self, dir: &str) -> Result<String> {
        Ok(hash(
            &serde_json::to_vec(&snapshot(Path::new(dir))?).map_err(e)?,
        ))
    }
    pub fn prepare_cc(&self, cc_dir: String, target: String) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        let dir = Path::new(&cc_dir).canonicalize().map_err(e)?;
        let cc = snapshot(&dir)?;
        let mut p = self.admin_plan("cc-migration")?;
        p.cc_check = Some((
            dir.to_string_lossy().into(),
            hash(&serde_json::to_vec(&cc).map_err(e)?),
        ));
        let selected = if target == "keep" {
            None
        } else {
            Some(self.storage_target(&target)?)
        };
        if let Some(base) = &selected {
            p.after.config.library = base.to_string_lossy().into();
        }
        let mut groups = BTreeMap::new();
        for g in &cc.groups {
            let old = string(g, "id");
            let name = string(g, "name");
            let id = p
                .after
                .config
                .groups
                .iter()
                .find(|x| x.name.eq_ignore_ascii_case(&name))
                .map(|x| x.id.clone())
                .unwrap_or_else(|| format!("cc-{}", hash(old.as_bytes())));
            if !p.after.config.groups.iter().any(|x| x.id == id) {
                p.after.config.groups.push(Group {
                    id: id.clone(),
                    name,
                    color: string(g, "color"),
                });
            }
            groups.insert(old, id);
        }
        for repo in &cc.repos {
            let r = Repo {
                repo: format!("{}/{}", string(repo, "owner"), string(repo, "name")),
                reference: string(repo, "branch"),
                enabled: enabled(repo, "enabled"),
            };
            if registry::validate_repo(&r.repo, &r.reference).is_ok()
                && !p.after.config.repos.iter().any(|x| x.repo == r.repo)
            {
                p.after.config.repos.push(r)
            }
        }
        for (client, key) in [
            ("Claude", "claudeConfigDir"),
            ("Codex", "codexConfigDir"),
            ("OpenCode", "opencodeConfigDir"),
        ] {
            let custom = string(&cc.settings, key);
            if !custom.is_empty() {
                let path = PathBuf::from(&custom).join("skills");
                p.warnings.push(format!(
                    "{client} 使用自定义目录 {}；请先在客户端目录设置中填入，再重新预览迁移",
                    path.display()
                ));
                if self.config()?.clients.get(client) != Some(&path.to_string_lossy().to_string()) {
                    return Err(format!(
                        "检测到 {client} 自定义目录 {}，请先配置对应客户端目录再迁移",
                        path.display()
                    ));
                }
            }
        }
        let unified = string(&cc.settings, "skillStorageLocation") == "unified";
        let bases = if unified {
            vec![self.home.join(".agents/skills"), dir.join("skills")]
        } else {
            vec![dir.join("skills"), self.home.join(".agents/skills")]
        };
        for v in &cc.rows {
            let key = format!("{}:{}", dir.display(), string(v, "id"));
            if p.after.config.imported.contains(&key) {
                p.summary
                    .push(format!("已迁入，跳过 {}", string(v, "name")));
                continue;
            }
            let directory = string(v, "directory");
            registry::relative(&directory)?;
            let candidates: Vec<_> = bases
                .iter()
                .map(|base| base.join(&directory))
                .filter(|path| path.join("SKILL.md").is_file())
                .collect();
            let source = if let Some(source) = candidates.first() {
                source.canonicalize().map_err(e)?
            } else {
                p.warnings
                    .push(format!("{}：元数据存在，但源目录缺失，未迁移", directory));
                continue;
            };
            if candidates.len() > 1
                && candidates[0].canonicalize().ok() != candidates[1].canonicalize().ok()
            {
                p.warnings.push(format!(
                    "{directory} 在两个源目录都存在，按 cc-switch 存储设置选择 {}",
                    source.display()
                ));
            }
            let name = source
                .file_name()
                .ok_or("缺少 Skill 目录名")?
                .to_string_lossy()
                .to_string();
            validate_name(&name)?;
            let id = hash(source.to_string_lossy().as_bytes());
            let expected = CLIENTS
                .iter()
                .filter_map(|(c, _)| {
                    let key = format!("enabled_{}", c.to_lowercase());
                    enabled(v, &key).then_some(c.to_string())
                })
                .collect();
            let record = Record {
                id: id.clone(),
                name: name.clone(),
                source: source.to_string_lossy().into(),
                download_source: origin(v).map(|origin| DownloadSource::Remote { origin }),
                origin: origin(v),
                baseline: if legacy_hash(&source).ok().as_ref() == Some(&string(v, "content_hash"))
                {
                    digest(&source)?
                } else {
                    format!("cc-unverified:{}", string(v, "content_hash"))
                },
                expected,
            };
            if record.origin.is_none() && !string(v, "repo_owner").is_empty() {
                p.warnings.push(format!("{name} 的仓库子路径无法从原元数据可靠解析；已保留原记录，迁入后需要关联精确来源"));
            }
            if let Some(old) = p.after.records.get(&id) {
                if old.origin.is_some() && old.origin != record.origin {
                    return Err(format!("{name} 已有不同升级来源，不会覆盖当前管理信息"));
                }
            }
            p.after.config.legacy.insert(id.clone(), v.clone());
            if let Some(group) = groups.get(&string(v, "group_id")) {
                p.after.config.members.insert(id.clone(), group.clone());
            }
            if let Some(base) = &selected {
                self.migrate_record(&mut p, record, base, false)?;
            } else {
                p.after.records.insert(id, record);
                p.summary
                    .push(format!("接管元数据：{name}（保留当前源和链接）"));
            }
            p.after.config.imported.insert(key);
        }
        // Import historical uninstall backups into independent storage, retaining the original backups.
        let backup_root = dir.join("skill-backups");
        if backup_root.is_dir() {
            for ent in fs::read_dir(backup_root).map_err(e)? {
                let path = ent.map_err(e)?.path();
                if !path.join("meta.json").is_file() || !path.join("skill/SKILL.md").is_file() {
                    continue;
                }
                let key = format!("backup:{}", path.display());
                if p.after.config.imported.contains(&key) {
                    continue;
                }
                let meta: serde_json::Value = read(&path.join("meta.json"))?;
                let v = &meta["skill"];
                let name = string(v, "directory");
                if validate_name(&name).is_err() {
                    p.warnings
                        .push(format!("跳过目录名无效的旧备份：{}", path.display()));
                    continue;
                }
                let source = selected
                    .clone()
                    .unwrap_or_else(|| self.library())
                    .join(&name);
                let token = token();
                let stage = self.admin_dir(&p.token).join(format!("cc-backup-{token}"));
                fs::create_dir_all(&stage).map_err(e)?;
                copy_tree(&path.join("skill"), &stage.join("content"))?;
                let origin_value = serde_json::json!({"repo_owner":v["repoOwner"],"repo_name":v["repoName"],"repo_branch":v["repoBranch"],"readme_url":v["readmeUrl"]});
                let r = Record {
                    id: hash(source.to_string_lossy().as_bytes()),
                    name: name.clone(),
                    source: source.to_string_lossy().into(),
                    download_source: origin(&origin_value)
                        .map(|origin| DownloadSource::Remote { origin }),
                    origin: origin(&origin_value),
                    baseline: digest(&stage.join("content"))?,
                    expected: CLIENTS
                        .iter()
                        .filter_map(|(c, _)| {
                            enabled(&v["apps"], &c.to_lowercase()).then_some(c.to_string())
                        })
                        .collect(),
                };
                p.after
                    .config
                    .legacy
                    .entry(r.id.clone())
                    .or_insert(v.clone());
                let b = Backup {
                    token: token.clone(),
                    id: r.id.clone(),
                    name,
                    created: meta["backupCreatedAt"].as_u64().unwrap_or(0),
                    digest: r.baseline.clone(),
                    previous: r,
                };
                atomic(&stage.join("backup.json"), &b)?;
                // Move files separately: safe_tree rejects links escaping a skill root, and the source snapshot remains untouched.
                let dest = self.data.join("backups").join(&token);
                self.checked_move(&mut p, &stage, &dest)?;
                p.summary.push(format!("复制旧备份：{}", path.display()));
                p.after.config.imported.insert(key);
            }
        }
        if cc.rows.is_empty() {
            p.warnings
                .push("cc-switch 中没有已登记的 Skill；可使用本机未管理技能接管".into())
        }
        p.summary.push(format!(
            "迁入 {} 个分组、{} 个仓库配置；不修改 cc-switch 数据库",
            cc.groups.len(),
            cc.repos.len()
        ));
        self.finish_plan(p)
    }
    pub fn set_client_path(&self, client: String, path: String) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        if !CLIENTS.iter().any(|(c, _)| *c == client) {
            return Err("未知客户端".into());
        }
        let mut s = self.state()?;
        if s.records.values().any(|r| r.expected.contains(&client)) {
            return Err("请先关闭该客户端已管理的投影，再更换路径".into());
        }
        if path.is_empty() {
            s.config.clients.remove(&client);
        } else {
            let p = PathBuf::from(&path);
            if !p.is_absolute() {
                return Err("客户端目录需要绝对路径".into());
            }
            s.config.clients.insert(client, path);
        }
        self.save_state(&s)
    }
}
fn legacy_hash(root: &Path) -> Result<String> {
    safe_tree(root)?;
    let mut files = vec![];
    fn walk(base: &Path, p: &Path, files: &mut Vec<PathBuf>, depth: usize) -> Result<()> {
        if depth > 40 {
            return Err("旧版本哈希路径嵌套过深".into());
        }
        for entry in fs::read_dir(p).map_err(e)? {
            let entry = entry.map_err(e)?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, files, depth + 1)?;
            } else {
                files.push(path.strip_prefix(base).map_err(e)?.to_path_buf());
            }
        }
        Ok(())
    }
    walk(root, root, &mut files, 0)?;
    files.sort();
    let mut h = Sha256::new();
    for path in files {
        h.update(path.to_string_lossy().replace('\\', "/").as_bytes());
        h.update(b"\0");
        h.update(fs::read(root.join(&path)).map_err(e)?);
        h.update(b"\0");
    }
    Ok(format!("{:x}", h.finalize()))
}
