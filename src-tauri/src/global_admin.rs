//! Reviewable global administration: one filesystem journal and one SQLite state snapshot.
use super::*;
#[path = "global_cc.rs"]
mod cc;
#[path = "global_duplicates.rs"]
mod duplicates;
#[path = "global_repair.rs"]
mod repair;
pub use duplicates::{DuplicateChoice, DuplicateReport};

#[derive(Deserialize)]
pub struct PresetSyncChange {
    pub id: String,
    pub client: String,
    pub enable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    pub color: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Repo {
    pub repo: String,
    pub reference: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Projection {
    pub source: String,
    pub digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub library: String,
    pub sync_method: String,
    pub clients: BTreeMap<String, String>,
    pub groups: Vec<Group>,
    pub members: BTreeMap<String, String>,
    pub repos: Vec<Repo>,
    pub copies: BTreeMap<String, Projection>,
    pub imported: BTreeSet<String>,
    pub legacy: BTreeMap<String, serde_json::Value>,
    pub aliases: BTreeMap<String, String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            library: String::new(),
            sync_method: "symlink".into(),
            clients: BTreeMap::new(),
            groups: vec![],
            members: BTreeMap::new(),
            repos: vec![],
            copies: BTreeMap::new(),
            imported: BTreeSet::new(),
            legacy: BTreeMap::new(),
            aliases: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct State {
    pub records: BTreeMap<String, Record>,
    pub config: Config,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Move {
    pub from: PathBuf,
    pub to: PathBuf,
    pub fingerprint: String,
    #[serde(default)]
    pub parents: BTreeMap<PathBuf, PathBuf>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminPlan {
    pub token: String,
    pub kind: String,
    pub summary: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub cc_check: Option<(String, String)>,
    pub moves: Vec<Move>,
    pub checks: BTreeMap<PathBuf, Option<String>>,
    #[serde(default)]
    pub duplicate_checks: BTreeMap<PathBuf, String>,
    #[serde(default)]
    pub broken_checks: Vec<PathBuf>,
    pub before: State,
    pub after: State,
}
#[derive(Serialize, Deserialize)]
struct AdminJournal {
    plan: AdminPlan,
    completed: usize,
    in_flight: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminOverview {
    pub config: Config,
    pub client_defaults: BTreeMap<String, String>,
    pub backups: Vec<Backup>,
    pub cc_dir: String,
    pub history: Vec<AdminPlan>,
}
fn fingerprint(p: &Path) -> Result<String> {
    let meta = fs::symlink_metadata(p).map_err(e)?;
    if meta.file_type().is_symlink() {
        Ok(format!(
            "link:{}",
            fs::read_link(p).map_err(e)?.to_string_lossy()
        ))
    } else if meta.is_file() {
        Ok(format!("file:{}", hash(&fs::read(p).map_err(e)?)))
    } else {
        Ok(format!("tree:{}", digest(p)?))
    }
}
impl Manager {
    pub fn config(&self) -> Result<Config> {
        use rusqlite::OptionalExtension;
        let raw: Option<String> = self
            .db
            .query_row("SELECT value FROM admin WHERE key='config'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(e)?;
        raw.map(|r| serde_json::from_str(&r).map_err(e))
            .transpose()
            .map(|x| x.unwrap_or_default())
    }
    fn state(&self) -> Result<State> {
        Ok(State {
            records: self.records()?,
            config: self.config()?,
        })
    }
    fn save_state(&self, s: &State) -> Result<()> {
        let tx = self.db.unchecked_transaction().map_err(e)?;
        tx.execute("DELETE FROM skills", []).map_err(e)?;
        for r in s.records.values() {
            tx.execute(
                "INSERT INTO skills(id,record) VALUES(?1,?2)",
                params![r.id, serde_json::to_string(r).map_err(e)?],
            )
            .map_err(e)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO admin(key,value) VALUES('config',?1)",
            [serde_json::to_string(&s.config).map_err(e)?],
        )
        .map_err(e)?;
        tx.commit().map_err(e)
    }
    fn admin_plan(&self, kind: &str) -> Result<AdminPlan> {
        self.ready()?;
        let before = self.state()?;
        let p = AdminPlan {
            token: token(),
            kind: kind.into(),
            summary: vec![],
            warnings: vec![],
            cc_check: None,
            moves: vec![],
            checks: BTreeMap::new(),
            duplicate_checks: BTreeMap::new(),
            broken_checks: vec![],
            after: before.clone(),
            before,
        };
        fs::create_dir_all(self.admin_dir(&p.token)).map_err(e)?;
        Ok(p)
    }
    fn admin_dir(&self, token: &str) -> PathBuf {
        self.data.join("admin-plans").join(token)
    }
    fn checked_move(&self, p: &mut AdminPlan, from: &Path, to: &Path) -> Result<()> {
        let f = fingerprint(from)?;
        self.move_with_hash(p, from, to, f)
    }
    fn move_with_hash(&self, p: &mut AdminPlan, from: &Path, to: &Path, f: String) -> Result<()> {
        if from == to {
            return Ok(());
        }
        for path in [from, to] {
            if !p.moves.iter().any(|m| m.from == path || m.to == path) {
                p.checks.insert(
                    path.into(),
                    if exists(path) {
                        Some(fingerprint(path)?)
                    } else {
                        None
                    },
                );
            }
        }
        let currently_occupied = exists(to) && !p.moves.iter().any(|m| m.from == to);
        let scheduled_occupied =
            p.moves.iter().any(|m| m.to == to) && !p.moves.iter().any(|m| m.from == to);
        if currently_occupied || scheduled_occupied {
            return Err(format!("目标已存在，不会覆盖：{}", to.display()));
        }
        let mut parents = BTreeMap::new();
        for leaf in [from, to] {
            let mut parent = leaf.parent().ok_or("路径没有父目录")?;
            while !parent.exists() {
                parent = parent.parent().ok_or("没有现存父目录")?;
            }
            parents.insert(parent.into(), parent.canonicalize().map_err(e)?);
        }
        p.moves.push(Move {
            from: from.into(),
            to: to.into(),
            fingerprint: f,
            parents,
        });
        Ok(())
    }
    fn finish_plan(&self, p: AdminPlan) -> Result<AdminPlan> {
        atomic(&self.admin_dir(&p.token).join("plan.json"), &p)?;
        Ok(p)
    }
    pub fn apply_admin(&self, token: &str) -> Result<()> {
        self.apply_admin_with_failure(token, None)
    }
    #[doc(hidden)]
    pub fn apply_admin_with_failure(&self, token: &str, fail_at: Option<usize>) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        valid_token(token)?;
        let p: AdminPlan = read(&self.admin_dir(token).join("plan.json"))?;
        if p.token != token || self.state()? != p.before {
            return Err("预览后的管理信息已变化，请重新预览".into());
        }
        if let Some((dir, expected)) = &p.cc_check {
            if self.cc_fingerprint(dir)? != *expected {
                return Err("cc-switch 的 Skill 信息在预览后发生变化，请重新预览".into());
            }
        }
        for path in &p.broken_checks {
            repair::require_broken(path)?;
        }
        for (path, expected) in &p.duplicate_checks {
            if p.kind == "repair-links" && path.canonicalize().map_err(e)? != *path {
                return Err("修复目标的实际目录已变化，请重新预览".into());
            }
            if duplicates::duplicate_digest(path)? != *expected {
                return Err(format!(
                    "Skill 的内容或权限已变化，请重新预览：{}",
                    path.display()
                ));
            }
        }
        for (path, expected) in &p.checks {
            let actual = if exists(path) {
                Some(fingerprint(path)?)
            } else {
                None
            };
            if &actual != expected {
                return Err(format!("预览后的文件已变化：{}", path.display()));
            }
        }
        let mut journal = AdminJournal {
            plan: p.clone(),
            completed: 0,
            in_flight: false,
        };
        atomic(&self.data.join("admin-transaction.json"), &journal)?;
        let result: Result<()> = (|| {
            for (i, m) in p.moves.iter().enumerate() {
                if p.broken_checks.contains(&m.from) {
                    repair::require_broken(&m.from)?;
                }
                for (parent, canonical) in &m.parents {
                    if parent.canonicalize().map_err(e)? != *canonical {
                        return Err("父目录在操作期间发生变化".into());
                    }
                }
                if fingerprint(&m.from)? != m.fingerprint {
                    return Err(format!("移动前内容改变：{}", m.from.display()));
                }
                fs::create_dir_all(m.to.parent().ok_or("目标缺少父目录")?).map_err(e)?;
                journal.in_flight = true;
                atomic(&self.data.join("admin-transaction.json"), &journal)?;
                rename(&m.from, &m.to)?;
                journal.completed = i + 1;
                journal.in_flight = false;
                atomic(&self.data.join("admin-transaction.json"), &journal)?;
                if fingerprint(&m.to)? != m.fingerprint {
                    return Err("移动期间内容改变，已保留数据".into());
                }
                if fail_at == Some(i + 1) {
                    return Err("注入的管理操作故障".into());
                }
            }
            self.save_state(&p.after)?;
            atomic(&self.admin_dir(token).join("completed.json"), &p)?;
            fs::remove_file(self.data.join("admin-transaction.json")).map_err(e)?;
            Ok(())
        })();
        if let Err(error) = result {
            return match self.rollback_admin(&journal) {
                Ok(()) => Err(format!("操作已回滚：{error}")),
                Err(r) => Err(format!("操作中断：{error}；需要恢复：{r}")),
            };
        }
        Ok(())
    }
    fn rollback_admin(&self, j: &AdminJournal) -> Result<()> {
        let p = &j.plan;
        let mut count = j.completed;
        if j.in_flight {
            let m = &p.moves[count];
            if !exists(&m.from) && exists(&m.to) {
                count += 1;
            } else if fingerprint(&m.from)? != m.fingerprint {
                return Err("中断步骤的源已变化".into());
            }
        }
        for (index, m) in p.moves[..count].iter().enumerate().rev() {
            // A later move can consume this target; reverse order reconstructs it.
            if exists(&m.to) {
                if exists(&m.from) || fingerprint(&m.to)? != m.fingerprint {
                    return Err(format!("恢复冲突，保留双方：{}", m.to.display()));
                }
                fs::create_dir_all(m.from.parent().ok_or("来源缺少父目录")?).map_err(e)?;
                rename(&m.to, &m.from)?;
            } else if !exists(&m.from) || fingerprint(&m.from)? != m.fingerprint {
                return Err(format!("原资源丢失或改变：{}", m.from.display()));
            }
            atomic(
                &self.data.join("admin-transaction.json"),
                &AdminJournal {
                    plan: p.clone(),
                    completed: index,
                    in_flight: false,
                },
            )?;
        }
        self.save_state(&p.before)?;
        fs::remove_file(self.data.join("admin-transaction.json")).map_err(e)?;
        Ok(())
    }
    pub fn recover_admin(&self) -> Result<()> {
        let _lock = self.lock()?;
        let path = self.data.join("admin-transaction.json");
        if path.exists() {
            self.rollback_admin(&read(&path)?)?;
        }
        Ok(())
    }
    pub fn undo_admin(&self, token: &str) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        valid_token(token)?;
        let old: AdminPlan = read(&self.admin_dir(token).join("completed.json"))?;
        if self.state()? != old.after {
            return Err("此操作之后已有新的管理变更，不能直接回退整批迁移".into());
        }
        let mut p = self.admin_plan("undo")?;
        p.after = old.before;
        p.summary = vec![format!("回退 {}（{} 项移动）", old.kind, old.moves.len())];
        for m in old.moves.iter().rev() {
            self.move_with_hash(&mut p, &m.to, &m.from, m.fingerprint.clone())?;
        }
        self.finish_plan(p)
    }
    pub fn overview(&self) -> Result<AdminOverview> {
        let mut backups = vec![];
        let root = self.data.join("backups");
        if root.exists() {
            for e1 in fs::read_dir(root).map_err(e)? {
                let path = e1.map_err(e)?.path();
                if path.join("backup.json").is_file() && path.join("content").exists() {
                    backups.push(read(&path.join("backup.json"))?);
                }
            }
        }
        let mut history = vec![];
        let root = self.data.join("admin-plans");
        if root.exists() {
            for ent in fs::read_dir(root).map_err(e)? {
                let path = ent.map_err(e)?.path().join("completed.json");
                if path.is_file() && !path.with_file_name("history-hidden.json").exists() {
                    let p: AdminPlan = read(&path)?;
                    if [
                        "cc-migration",
                        "storage",
                        "adopt",
                        "uninstall",
                        "sync",
                        "deduplicate",
                        "repair-links",
                        "undo",
                    ]
                    .contains(&p.kind.as_str())
                    {
                        history.push(p);
                    }
                }
            }
        }
        history.sort_by(|a, b| b.token.cmp(&a.token));
        history.truncate(20);
        Ok(AdminOverview {
            config: self.config()?,
            client_defaults: CLIENTS
                .iter()
                .map(|(client, path)| {
                    (
                        (*client).into(),
                        self.home.join(path).to_string_lossy().into_owned(),
                    )
                })
                .collect(),
            backups,
            cc_dir: self.home.join(".cc-switch").to_string_lossy().into(),
            history,
        })
    }
    pub fn set_history_deleted(&self, token: &str, deleted: bool) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        valid_token(token)?;
        let root = self.data.join("admin-plans");
        let dir = self.admin_dir(token);
        let completed = dir.join("completed.json");
        for path in [&root, &dir, &completed] {
            if fs::symlink_metadata(path)
                .map_err(e)?
                .file_type()
                .is_symlink()
            {
                return Err("历史记录路径无效".into());
            }
        }
        let plan: AdminPlan = read(&completed)?;
        if plan.token != token {
            return Err("历史记录标识不匹配".into());
        }
        // Completed plans and their payloads must survive cache cleanup and undo.
        let marker = dir.join("history-hidden.json");
        if deleted {
            atomic(&marker, &true)
        } else {
            match fs::remove_file(marker) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(e(error)),
            }
        }
    }
    pub fn save_group(
        &self,
        id: Option<String>,
        name: String,
        color: String,
        members: Option<Vec<String>>,
    ) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        let mut s = self.state()?;
        if name.trim().is_empty() || name.len() > 100 {
            return Err("分组名需要 1–100 字符".into());
        }
        if ![
            "blue", "violet", "emerald", "amber", "rose", "cyan", "slate",
        ]
        .contains(&color.as_str())
        {
            return Err("未知分组颜色".into());
        }
        let id = id.unwrap_or_else(token);
        if s.config
            .groups
            .iter()
            .any(|g| g.id != id && g.name.to_lowercase() == name.trim().to_lowercase())
        {
            return Err("已有同名分组".into());
        }
        let group = Group {
            id: id.clone(),
            name: name.trim().into(),
            color,
        };
        if let Some(existing) = s.config.groups.iter_mut().find(|g| g.id == id) {
            *existing = group;
        } else {
            s.config.groups.push(group);
        }
        if let Some(members) = members {
            s.config.members.retain(|_, g| g != &id);
            for member in members {
                let r = self.record(&member)?;
                s.records.insert(member.clone(), r);
                s.config.members.insert(member, id.clone());
            }
        }
        self.save_state(&s)
    }
    pub fn reorder_groups(&self, ids: Vec<String>) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        let mut s = self.state()?;
        let existing: BTreeSet<_> = s.config.groups.iter().map(|g| &g.id).collect();
        let requested: BTreeSet<_> = ids.iter().collect();
        if ids.len() != s.config.groups.len() || requested != existing {
            return Err("分组列表已变化，请刷新后重新排序".into());
        }
        let mut groups: BTreeMap<_, _> = s
            .config
            .groups
            .into_iter()
            .map(|g| (g.id.clone(), g))
            .collect();
        s.config.groups = ids.iter().map(|id| groups.remove(id).unwrap()).collect();
        self.save_state(&s)
    }
    pub fn move_group(&self, ids: Vec<String>, group: Option<String>) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        let mut s = self.state()?;
        if group
            .as_ref()
            .is_some_and(|g| !s.config.groups.iter().any(|x| &x.id == g))
        {
            return Err("分组不存在".into());
        }
        for id in ids {
            let r = self.record(&id)?;
            s.records.insert(id.clone(), r);
            s.config.members.remove(&id);
            if let Some(g) = &group {
                s.config.members.insert(id, g.clone());
            }
        }
        self.save_state(&s)
    }
    pub fn delete_group(&self, id: &str) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        let mut s = self.state()?;
        s.config.groups.retain(|g| g.id != id);
        s.config.members.retain(|_, g| g != id);
        self.save_state(&s)
    }
    pub fn save_repo(&self, mut repo: Repo, remove: bool) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        repo = parse_repo_input(repo)?;
        registry::validate_repo(&repo.repo, &repo.reference)?;
        let mut s = self.state()?;
        s.config.repos.retain(|r| r.repo != repo.repo);
        if !remove {
            s.config.repos.push(repo)
        }
        self.save_state(&s)
    }
    pub fn discover_repos(&self) -> Result<Vec<std::result::Result<Discovery, String>>> {
        self.config()?
            .repos
            .into_iter()
            .filter(|r| r.enabled)
            .map(|r| Ok(self.discover(&r.repo, &r.reference)))
            .collect()
    }
    pub fn inspect_archive(&self, path: &Path) -> Result<Discovery> {
        let _lock = self.lock()?;
        self.ready()?;
        if ![Some("zip"), Some("skill")].contains(&path.extension().and_then(|x| x.to_str())) {
            return Err("请选择 ZIP 或 .skill 文件".into());
        }
        let mut data = vec![];
        File::open(path)
            .map_err(e)?
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map_err(e)?;
        if data.len() > 64 * 1024 * 1024 {
            return Err("归档超过 64 MB".into());
        }
        let temp = tempfile::tempdir().map_err(e)?;
        registry::extract_local_archive(&data, temp.path())?;
        let discovery = self.cache_discovery_named(
            "local/archive",
            "HEAD",
            temp.path(),
            path.file_stem().and_then(|s| s.to_str()).unwrap_or("skill"),
        )?;
        atomic(
            &self
                .data
                .join("discoveries")
                .join(&discovery.token)
                .join("download-source.json"),
            &DownloadSource::Local {
                path: path.canonicalize().map_err(e)?.to_string_lossy().into(),
            },
        )?;
        Ok(discovery)
    }
    pub fn prepare_archive(&self, discovery: &str, paths: Vec<String>) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        valid_token(discovery)?;
        let mut p = self.admin_plan("archive")?;
        let root = self.data.join("discoveries").join(discovery);
        let d: Discovery = read(&root.join("discovery.json"))?;
        if d.repo != "local/archive" {
            return Err("不是本地归档预览".into());
        }
        for path in paths {
            let c = d
                .candidates
                .iter()
                .find(|c| c.path == path)
                .ok_or("归档资源路径无效")?;
            let source = root.join("tree").join(&path);
            let dest = self.library().join(&c.name);
            let source = source.canonicalize().map_err(e)?;
            let stage = self
                .admin_dir(&p.token)
                .join(format!("payload-{}", p.moves.len()));
            copy_tree(&source, &stage)?;
            self.checked_move(&mut p, &stage, &dest)?;
            let r = Record {
                id: hash(dest.to_string_lossy().as_bytes()),
                name: c.name.clone(),
                source: dest.to_string_lossy().into(),
                origin: None,
                download_source: Some(if root.join("download-source.json").exists() {
                    read(&root.join("download-source.json"))?
                } else {
                    DownloadSource::Unknown
                }),
                baseline: digest(&source)?,
                expected: BTreeSet::new(),
            };
            p.summary
                .push(format!("安装 {} → {}", c.path, dest.display()));
            p.after.records.insert(r.id.clone(), r);
        }
        if p.moves.is_empty() {
            return Err("请选择至少一个 Skill".into());
        }
        self.finish_plan(p)
    }
}
impl Manager {
    fn project(
        &self,
        p: &mut AdminPlan,
        r: &Record,
        client: &str,
        enable: bool,
        method: &str,
        payload: Option<&Path>,
    ) -> Result<()> {
        let dest = self.destination(client, &r.name)?;
        if p.moves.iter().any(|m| m.to == dest || m.from == dest) {
            return Ok(());
        }
        let source = Path::new(&r.source);
        let current = self.cell(source, client, &r.name);
        if current == "source" {
            return if enable {
                Ok(())
            } else {
                Err(format!("{client} 是实体源目录，请先接管到共享库"))
            };
        }
        if !["off", "link", "copy"].contains(&current.as_str()) {
            return Err(format!(
                "{client} / {} 存在同名冲突或修改，不会覆盖",
                r.name
            ));
        }
        if exists(&dest) {
            let backup = self
                .admin_dir(&p.token)
                .join(format!("old-client-{}", p.moves.len()));
            self.checked_move(p, &dest, &backup)?;
        }
        p.after
            .config
            .copies
            .remove(&dest.to_string_lossy().to_string());
        if enable {
            let stage = self
                .admin_dir(&p.token)
                .join(format!("new-client-{}", p.moves.len()));
            if method == "copy" {
                let payload = payload.unwrap_or(source);
                copy_tree(payload, &stage)?;
                p.after.config.copies.insert(
                    dest.to_string_lossy().into(),
                    Projection {
                        source: r.source.clone(),
                        digest: digest(payload)?,
                    },
                );
            } else {
                create_link(source, &stage)?;
            }
            self.checked_move(p, &stage, &dest)?;
        }
        p.summary.push(format!(
            "{} · {client} → {}",
            r.name,
            if enable { method } else { "关闭，保留源" }
        ));
        Ok(())
    }
    /// Fill verified provenance without replacing skill files or changing groups.
    /// Uses the administration journal so the metadata change can be undone.
    pub fn prepare_download_sources(
        &self,
        sources: BTreeMap<String, DownloadSource>,
    ) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        if sources.is_empty() {
            return Err("请选择需要补充来源的 Skill".into());
        }
        let mut p = self.admin_plan("download-sources")?;
        for (id, source) in sources {
            let mut r = self.record(&id)?;
            if r.initial_source() != DownloadSource::Unknown {
                return Err(format!("{} 已有下载来源，不会覆盖", r.name));
            }
            match &source {
                DownloadSource::Remote { origin } => {
                    registry::validate_repo(&origin.repo, &origin.reference)?;
                    registry::relative(&origin.path)?;
                }
                DownloadSource::Local { path } => {
                    if !Path::new(path).is_absolute() || !Path::new(path).join("SKILL.md").is_file() {
                        return Err("本地来源必须是包含 SKILL.md 的绝对目录".into());
                    }
                }
                DownloadSource::Unknown => return Err("请提供已核实的下载来源".into()),
            }
            let path = PathBuf::from(&r.source);
            p.checks.insert(path.clone(), Some(fingerprint(&path)?));
            p.summary.push(format!("补充 {} 的下载来源", r.name));
            r.download_source = Some(source);
            p.after.records.insert(id, r);
        }
        self.finish_plan(p)
    }
    pub fn prepare_sync(
        &self,
        ids: Vec<String>,
        clients: Vec<String>,
        enable: bool,
        method: Option<String>,
    ) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        let mut p = self.admin_plan("sync")?;
        let method = method.unwrap_or(p.after.config.sync_method.clone());
        if !["copy", "symlink"].contains(&method.as_str()) {
            return Err("未知同步方式".into());
        }
        for id in ids {
            let mut r = self.record(&id)?;
            for c in &clients {
                self.project(&mut p, &r, c, enable, &method, None)?;
                if enable {
                    r.expected.insert(c.clone());
                } else {
                    r.expected.remove(c);
                }
            }
            p.after.records.insert(id, r);
        }
        self.finish_plan(p)
    }
    pub fn prepare_preset_sync(&self, changes: Vec<PresetSyncChange>) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        if changes.is_empty() {
            return Err("方案没有需要修改的资源".into());
        }
        let mut p = self.admin_plan("sync")?;
        let method = p.after.config.sync_method.clone();
        let mut seen = BTreeSet::new();
        let mut destinations = BTreeSet::new();
        for change in changes {
            if !seen.insert((change.id.clone(), change.client.clone())) {
                return Err("方案包含重复的客户端资源".into());
            }
            let mut r = p
                .after
                .records
                .get(&change.id)
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| self.record(&change.id))?;
            if !destinations.insert(self.destination(&change.client, &r.name)?) {
                return Err("方案中不同资源指向同一客户端目录，请先处理同名资源".into());
            }
            self.project(&mut p, &r, &change.client, change.enable, &method, None)?;
            if change.enable {
                r.expected.insert(change.client);
            } else {
                r.expected.remove(&change.client);
            }
            p.after.records.insert(change.id, r);
        }
        self.finish_plan(p)
    }
    pub fn set_sync_method(&self, method: String) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        if !["copy", "symlink"].contains(&method.as_str()) {
            return Err("未知同步方式".into());
        }
        let mut s = self.state()?;
        s.config.sync_method = method;
        self.save_state(&s)
    }
    fn migrate_record(
        &self,
        p: &mut AdminPlan,
        mut r: Record,
        base: &Path,
        adopt: bool,
    ) -> Result<()> {
        let source = PathBuf::from(&r.source);
        let dest = base.join(&r.name);
        let old_id = r.id.clone();
        let old_source = r.source.clone();
        if source == dest {
            p.after.records.insert(r.id.clone(), r);
            return Ok(());
        }
        let f = fingerprint(&source)?;
        let payload = self
            .admin_dir(&p.token)
            .join(format!("payload-copy-{}", p.moves.len()));
        // Copy-mode projections need a readable stage before the planned source move.
        copy_tree(&source, &payload)?;
        self.move_with_hash(p, &source, &dest, f)?;
        let new_record = Record {
            source: dest.to_string_lossy().into(),
            id: hash(dest.to_string_lossy().as_bytes()),
            ..r.clone()
        };
        let mut seen = BTreeSet::new();
        for (client, _) in CLIENTS {
            let client_dest = self.destination(client, &r.name)?;
            if !seen.insert(client_dest.clone()) {
                continue;
            }
            let mut status = self.cell(&source, client, &r.name);
            if status == "conflict"
                && r.expected.contains(*client)
                && fs::symlink_metadata(&client_dest).is_ok_and(|m| m.is_dir())
                && digest(&client_dest).ok() == digest(&source).ok()
            {
                status = "copy".into();
            }
            if status == "link" || status == "copy" || (status == "source" && adopt) {
                if client_dest != source {
                    let b = self
                        .admin_dir(&p.token)
                        .join(format!("previous-link-{}", p.moves.len()));
                    self.checked_move(p, &client_dest, &b)?;
                }
                let new = self
                    .admin_dir(&p.token)
                    .join(format!("replacement-{}", p.moves.len()));
                if status == "copy" && p.after.config.sync_method == "copy" {
                    copy_tree(&payload, &new)?;
                    p.after.config.copies.insert(
                        client_dest.to_string_lossy().into(),
                        Projection {
                            source: new_record.source.clone(),
                            digest: digest(&payload)?,
                        },
                    );
                } else {
                    create_link(&dest, &new)?;
                    p.after
                        .config
                        .copies
                        .remove(&client_dest.to_string_lossy().to_string());
                }
                self.checked_move(p, &new, &client_dest)?;
                r.expected.insert(client.to_string());
            } else if r.expected.contains(*client) {
                p.warnings.push(format!(
                    "{} · {client} 期望启用但当前为 {status}，未接管冲突路径",
                    r.name
                ));
            }
        }
        // Source aliases (for example OpenClaw) that are not controlled clients are reported.
        let mut new_record = new_record;
        new_record.expected = r.expected;
        for value in p.after.config.aliases.values_mut() {
            if value == &old_id {
                *value = new_record.id.clone();
            }
        }
        p.after
            .config
            .aliases
            .insert(old_id.clone(), new_record.id.clone());
        p.after.records.remove(&old_id);
        p.after
            .records
            .insert(new_record.id.clone(), new_record.clone());
        if let Some(g) = p.after.config.members.remove(&old_id) {
            p.after.config.members.insert(new_record.id.clone(), g);
        }
        if let Some(v) = p.after.config.legacy.remove(&old_id) {
            p.after.config.legacy.insert(new_record.id.clone(), v);
        }
        p.summary.push(format!(
            "{} → {}（客户端链接随源迁移）",
            old_source,
            dest.display()
        ));
        Ok(())
    }
    pub fn prepare_storage(
        &self,
        target: String,
        ids: Option<Vec<String>>,
        adopt: bool,
    ) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        let mut p = self.admin_plan(if adopt { "adopt" } else { "storage" })?;
        let base = self.storage_target(&target)?;
        let records = if let Some(ids) = ids {
            ids.iter()
                .map(|id| self.record(id))
                .collect::<Result<Vec<_>>>()?
        } else {
            p.before.records.values().cloned().collect()
        };
        for r in records {
            if !adopt {
                self.ensure_shared(Path::new(&r.source))?;
            }
            self.migrate_record(&mut p, r, &base, adopt)?;
        }
        if !adopt {
            p.after.config.library = base.to_string_lossy().into();
        }
        self.finish_plan(p)
    }
    fn storage_target(&self, target: &str) -> Result<PathBuf> {
        let base = match target {
            "independent" => self.home.join(".skill-manager/skills"),
            "unified" => self.home.join(".agents/skills"),
            "cc_switch" => self.home.join(".cc-switch/skills"),
            _ => return Err("未知存储位置".into()),
        };
        if base.exists() {
            base.canonicalize().map_err(e)
        } else {
            Ok(base)
        }
    }
    pub fn prepare_uninstall(&self, ids: Vec<String>) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        let mut p = self.admin_plan("uninstall")?;
        for id in ids {
            let r = self.record(&id)?;
            self.ensure_shared(Path::new(&r.source))?;
            for (client, _) in CLIENTS {
                if self.cell(Path::new(&r.source), client, &r.name) == "modified" {
                    let dest = self.destination(client, &r.name)?;
                    p.after
                        .config
                        .copies
                        .remove(&dest.to_string_lossy().to_string());
                    p.warnings.push(format!(
                        "保留 {client} 中修改过的 {} 副本，并解除该副本接管",
                        r.name
                    ));
                }
                if ["link", "copy"]
                    .contains(&self.cell(Path::new(&r.source), client, &r.name).as_str())
                {
                    self.project(&mut p, &r, client, false, "symlink", None)?;
                }
            }
            let backup_token = token();
            let base = self.data.join("backups").join(&backup_token);
            self.checked_move(&mut p, Path::new(&r.source), &base.join("content"))?;
            let b = Backup {
                token: backup_token,
                id: r.id.clone(),
                name: r.name.clone(),
                created: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(e)?
                    .as_secs(),
                previous: r.clone(),
                digest: digest(Path::new(&r.source))?,
            };
            let meta = self
                .admin_dir(&p.token)
                .join(format!("backup-meta-{}", p.moves.len()));
            atomic(&meta, &b)?;
            self.checked_move(&mut p, &meta, &base.join("backup.json"))?;
            p.after.records.remove(&id);
            p.summary
                .push(format!("卸载 {}，完整内容及原启用状态保留在备份", r.name));
        }
        self.finish_plan(p)
    }
    pub fn prepare_restore_removed(&self, token: &str, clients: Vec<String>) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        valid_token(token)?;
        let mut p = self.admin_plan("restore")?;
        let dir = self.data.join("backups").join(token);
        let b: Backup = read(&dir.join("backup.json"))?;
        if digest(&dir.join("content"))? != b.digest {
            return Err("备份内容已改变".into());
        }
        let dest = self.library().join(&b.name);
        let payload = self.admin_dir(&p.token).join("restored-source");
        copy_tree(&dir.join("content"), &payload)?;
        self.checked_move(&mut p, &payload, &dest)?;
        let r = Record {
            id: hash(dest.to_string_lossy().as_bytes()),
            source: dest.to_string_lossy().into(),
            expected: clients.iter().cloned().collect(),
            ..b.previous
        };
        for c in clients {
            let method = p.after.config.sync_method.clone();
            self.project(&mut p, &r, &c, true, &method, Some(&dir.join("content")))?;
        }
        p.summary.push(format!("恢复 {} → {}", r.name, r.source));
        p.after.records.insert(r.id.clone(), r);
        self.finish_plan(p)
    }
    pub fn delete_backup(&self, token: &str) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        valid_token(token)?;
        let dir = self.data.join("backups").join(token);
        let b: Backup = read(&dir.join("backup.json"))?;
        if b.token != token
            || fs::symlink_metadata(&dir)
                .map_err(e)?
                .file_type()
                .is_symlink()
        {
            return Err("备份路径无效".into());
        }
        fs::remove_dir_all(dir).map_err(e)
    }
    pub fn apply_content_admin(&self, token: &str, allow: bool) -> Result<()> {
        let p = {
            let _lock = self.lock()?;
            self.ready()?;
            valid_token(token)?;
            let original: Plan = read(&self.data.join("plans").join(token).join("plan.json"))?;
            self.ensure_shared(Path::new(&original.record.source))?;
            if self.records()?.get(&original.record.id) != original.before.as_ref() {
                return Err("预览后 Skill 管理状态已变化".into());
            }
            if original.local_modified && !allow {
                return Err("检测到本地修改，请确认备份后升级".into());
            }
            let source = Path::new(&original.record.source);
            let payload = self.data.join("plans").join(token).join("payload");
            if digest(&payload)? != original.new_hash {
                return Err("待安装内容已变化，请重新预览".into());
            }
            if original.old_hash.is_some() {
                if Some(digest(source)?) != original.old_hash
                    || fs::symlink_metadata(source)
                        .map_err(e)?
                        .file_type()
                        .is_symlink()
                {
                    return Err("预览后本地内容发生变化".into());
                }
            } else if exists(source) {
                return Err("安装目标出现同名内容".into());
            }
            let mut p = self.admin_plan("content")?;
            if let Some(old) = &original.old_hash {
                let bdir = self.data.join("backups").join(token);
                self.checked_move(&mut p, source, &bdir.join("content"))?;
                let previous = original.before.clone().unwrap_or(Record {
                    baseline: old.clone(),
                    origin: None,
                    ..original.record.clone()
                });
                let b = Backup {
                    token: token.into(),
                    id: original.record.id.clone(),
                    name: original.record.name.clone(),
                    created: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(e)?
                        .as_secs(),
                    previous,
                    digest: old.clone(),
                };
                let meta = self.admin_dir(&p.token).join("backup-meta");
                atomic(&meta, &b)?;
                self.checked_move(&mut p, &meta, &bdir.join("backup.json"))?;
            }
            self.checked_move(&mut p, &payload, source)?;
            for (client, _) in CLIENTS {
                if self.cell(source, client, &original.record.name) == "copy" {
                    self.project(
                        &mut p,
                        &original.record,
                        client,
                        true,
                        "copy",
                        Some(&payload),
                    )?;
                }
            }
            // Modified copies are not overwritten or treated as successfully synchronized.
            for (client, _) in CLIENTS {
                if self.cell(source, client, &original.record.name) == "modified" {
                    return Err(format!("{client} 副本有本地修改，请先保留该副本再升级"));
                }
            }
            p.after
                .records
                .insert(original.record.id.clone(), original.record);
            self.finish_plan(p)?
        };
        self.apply_admin(&p.token)
    }
}
impl Manager {
    pub fn admin_command(
        &self,
        action: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value> {
        fn get<T: serde::de::DeserializeOwned>(v: &serde_json::Value, k: &str) -> Result<T> {
            serde_json::from_value(v.get(k).cloned().unwrap_or(serde_json::Value::Null))
                .map_err(|e| format!("参数 {k}：{e}"))
        }
        fn out<T: Serialize>(v: T) -> Result<serde_json::Value> {
            serde_json::to_value(v).map_err(e)
        }
        match action {
            "overview" => out(self.overview()?),
            "broken_links" => out(self.broken_links(&get::<String>(&args, "targetRoot")?)?),
            "repair_links_preview" => out(self
                .prepare_link_repair(&get::<String>(&args, "targetRoot")?, get(&args, "paths")?)?),
            "duplicates" => out(self.duplicate_report()?),
            "plan_compare" => {
                out(self.compare_plan(&get::<String>(&args, "token")?, get(&args, "path")?)?)
            }
            "duplicate_compare" => out(self.compare_skills(
                &get::<String>(&args, "leftId")?,
                &get::<String>(&args, "rightId")?,
                get(&args, "path")?,
            )?),
            "duplicates_preview" => out(self.prepare_duplicates(get(&args, "choices")?)?),
            "cc_preview" => out(self.prepare_cc(get(&args, "ccDir")?, get(&args, "target")?)?),
            "storage_preview" => out(self.prepare_storage(
                get(&args, "target")?,
                get(&args, "ids")?,
                get(&args, "adopt")?,
            )?),
            "preset_preview" => out(self.prepare_preset_sync(get(&args, "changes")?)?),
            "sync_preview" => out(self.prepare_sync(
                get(&args, "ids")?,
                get(&args, "clients")?,
                get(&args, "enable")?,
                get(&args, "method")?,
            )?),
            "uninstall_preview" => out(self.prepare_uninstall(get(&args, "ids")?)?),
            "restore_preview" => out(self.prepare_restore_removed(
                &get::<String>(&args, "token")?,
                get(&args, "clients")?,
            )?),
            "undo_preview" => out(self.undo_admin(&get::<String>(&args, "token")?)?),
            "apply" => out(self.apply_admin(&get::<String>(&args, "token")?)?),
            "archive_inspect" => {
                out(self.inspect_archive(Path::new(&get::<String>(&args, "path")?))?)
            }
            "archive_preview" => {
                out(self
                    .prepare_archive(&get::<String>(&args, "discovery")?, get(&args, "paths")?)?)
            }
            "save_group" => out(self.save_group(
                get(&args, "id")?,
                get(&args, "name")?,
                get(&args, "color")?,
                get(&args, "members")?,
            )?),
            "move_group" => out(self.move_group(get(&args, "ids")?, get(&args, "group")?)?),
            "reorder_groups" => out(self.reorder_groups(get(&args, "ids")?)?),
            "delete_group" => out(self.delete_group(&get::<String>(&args, "id")?)?),
            "save_repo" => out(self.save_repo(get(&args, "repo")?, get(&args, "remove")?)?),
            "discover_repos" => out(self.discover_repos()?),
            "sync_method" => out(self.set_sync_method(get(&args, "method")?)?),
            "client_path" => out(self.set_client_path(get(&args, "client")?, get(&args, "path")?)?),
            "clear_cache" => out(self.clear_cache()?),
            "delete_backup" => out(self.delete_backup(&get::<String>(&args, "token")?)?),
            "delete_history" => {
                out(self.set_history_deleted(&get::<String>(&args, "token")?, true)?)
            }
            "restore_history" => {
                out(self.set_history_deleted(&get::<String>(&args, "token")?, false)?)
            }
            _ => Err("未知管理操作".into()),
        }
    }
}
pub fn parse_repo_input(mut r: Repo) -> Result<Repo> {
    r.repo = r.repo.trim().to_string();
    if r.repo.starts_with("ccswitch://") {
        let url = reqwest::Url::parse(&r.repo).map_err(e)?;
        let q: BTreeMap<_, _> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if q.get("resource").map(String::as_str) != Some("skill") {
            return Err("该分享链接不是 Skill 仓库".into());
        }
        r.repo = q.get("repo").cloned().ok_or("分享链接缺少 repo")?;
        r.reference = q.get("branch").cloned().unwrap_or_else(|| "main".into());
        r.enabled = q.get("enabled").is_none_or(|x| x != "false" && x != "0");
    } else if r.repo.starts_with("https://github.com/") {
        let url = reqwest::Url::parse(&r.repo).map_err(e)?;
        let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
        if parts.len() != 2 {
            return Err("请粘贴 GitHub 仓库首页链接，分支单独填写".into());
        }
        r.repo = format!("{}/{}", parts[0], parts[1].trim_end_matches(".git"));
    }
    registry::validate_repo(&r.repo, &r.reference)?;
    Ok(r)
}
impl Manager {
    pub fn clear_cache(&self) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        for name in ["discoveries", "plans"] {
            let path = self.data.join(name);
            if path.exists() {
                if fs::symlink_metadata(&path)
                    .map_err(e)?
                    .file_type()
                    .is_symlink()
                {
                    return Err("缓存目录不能是软链接".into());
                }
                fs::remove_dir_all(path).map_err(e)?;
            }
        }
        let root = self.data.join("admin-plans");
        if root.is_dir() {
            for ent in fs::read_dir(root).map_err(e)? {
                let path = ent.map_err(e)?.path();
                if !path.join("completed.json").exists()
                    && fs::symlink_metadata(&path).map_err(e)?.is_dir()
                {
                    fs::remove_dir_all(path).map_err(e)?;
                }
            }
        }
        Ok(())
    }
}
