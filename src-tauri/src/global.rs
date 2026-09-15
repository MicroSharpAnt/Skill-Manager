#[path = "global_admin.rs"]
pub mod admin;
#[path = "global_install_status.rs"]
pub mod install_status;
// Global source directories are shared through symlinks. Content replacement
// and link batches use an independent journal and never touch project state.
use crate::{
    engine::{frontmatter, Result},
    registry::{self, Candidate, Origin},
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub const CLIENTS: &[(&str, &str)] = &[
    ("Claude", ".claude/skills"),
    ("Codex", ".codex/skills"),
    ("DeepSeek Harness", ".dsh/skills"),
    ("OpenCode", ".config/opencode/skills"),
    ("ZCode", ".zcode/skills"),
    ("Kimi", ".kimi/skills"),
];
fn e(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn token() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}
fn valid_token(s: &str) -> Result<()> {
    if s.is_empty() || s.len() > 80 || !s.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return Err("无效操作编号".into());
    }
    Ok(())
}
fn atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", token()));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(e)?;
    f.write_all(&serde_json::to_vec_pretty(value).map_err(e)?)
        .and_then(|_| f.sync_all())
        .map_err(e)?;
    fs::rename(tmp, path).map_err(e)?;
    File::open(path.parent().unwrap())
        .and_then(|f| f.sync_all())
        .map_err(e)
}
fn read<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).map_err(e)?).map_err(e)
}
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 150
        || name.starts_with('.')
        || name.contains(['/', '\\', '\0'])
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || "-_.".contains(c))
    {
        return Err(format!("Skill 名称不可用于目录：{name}"));
    }
    Ok(())
}
#[cfg(unix)]
pub fn create_link(source: &Path, dest: &Path) -> Result<()> {
    std::os::unix::fs::symlink(source, dest).map_err(e)
}
#[cfg(not(unix))]
pub fn create_link(source: &Path, dest: &Path) -> Result<()> {
    std::os::windows::fs::symlink_dir(source, dest).map_err(e)
}
fn rename(source: &Path, dest: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(source.as_os_str().as_bytes()).map_err(e)?;
        let b = std::ffi::CString::new(dest.as_os_str().as_bytes()).map_err(e)?;
        if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) } != 0 {
            return Err(e(std::io::Error::last_os_error()));
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(source.as_os_str().as_bytes()).map_err(e)?;
        let b = std::ffi::CString::new(dest.as_os_str().as_bytes()).map_err(e)?;
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(e(std::io::Error::last_os_error()));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        return Err("当前平台暂不支持安全移动".into());
    }
    for p in [source.parent().unwrap(), dest.parent().unwrap()] {
        File::open(p).and_then(|f| f.sync_all()).map_err(e)?;
    }
    Ok(())
}
/// Permit internal relative symlinks; reject escapes, broken targets and special
/// files. Hash link targets without dereferencing, so cycles cannot recurse.
pub fn safe_tree(root: &Path) -> Result<BTreeMap<String, String>> {
    let root = root.canonicalize().map_err(e)?;
    let mut files = BTreeMap::new();
    let mut bytes = 0u64;
    fn visit(
        root: &Path,
        p: &Path,
        files: &mut BTreeMap<String, String>,
        bytes: &mut u64,
    ) -> Result<()> {
        if files.len() > 20000 {
            return Err("Skill 文件数量超过限制".into());
        }
        let meta = fs::symlink_metadata(p).map_err(e)?;
        let rel = p
            .strip_prefix(root)
            .map_err(e)?
            .to_string_lossy()
            .to_string();
        if meta.file_type().is_symlink() {
            let target = fs::read_link(p).map_err(e)?;
            if target.is_absolute()
                || !p
                    .canonicalize()
                    .map_err(|_| "Skill 含断开的软链接")?
                    .starts_with(root)
            {
                return Err(format!("Skill 含越界软链接：{rel}"));
            }
            files.insert(rel, format!("link:{}", target.to_string_lossy()));
        } else if meta.is_dir() {
            files.insert(format!("{rel}/"), "directory".into());
            for entry in fs::read_dir(p).map_err(e)? {
                let entry = entry.map_err(e)?;
                if entry.file_name() == ".git" {
                    return Err("Skill 内不能包含 Git 仓库数据".into());
                }
                visit(root, &entry.path(), files, bytes)?;
            }
        } else if meta.is_file() {
            *bytes += meta.len();
            if *bytes > 256 * 1024 * 1024 {
                return Err("Skill 内容超过 256 MB 限制".into());
            }
            let mut f = File::open(p).map_err(e)?;
            let mut h = Sha256::new();
            let mut buf = [0; 65536];
            loop {
                let n = f.read(&mut buf).map_err(e)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            files.insert(rel, format!("file:{executable}:{:x}", h.finalize()));
        } else {
            return Err("Skill 含特殊文件".into());
        }
        Ok(())
    }
    visit(&root, &root, &mut files, &mut bytes)?;
    Ok(files)
}
fn digest(root: &Path) -> Result<String> {
    Ok(hash(&serde_json::to_vec(&safe_tree(root)?).map_err(e)?))
}
fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    safe_tree(source)?;
    fn copy(a: &Path, b: &Path) -> Result<()> {
        let meta = fs::symlink_metadata(a).map_err(e)?;
        if meta.file_type().is_symlink() {
            create_link(&fs::read_link(a).map_err(e)?, b)?;
        } else if meta.is_dir() {
            fs::create_dir(b).map_err(e)?;
            for entry in fs::read_dir(a).map_err(e)? {
                let entry = entry.map_err(e)?;
                copy(&entry.path(), &b.join(entry.file_name()))?;
            }
            fs::set_permissions(b, meta.permissions()).map_err(e)?;
        } else {
            fs::copy(a, b).map_err(e)?;
        }
        Ok(())
    }
    copy(source, dest)?;
    if digest(source)? != digest(dest)? {
        return Err("复制后内容校验失败，源目录可能发生了变化".into());
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DownloadSource {
    Remote { origin: Origin },
    Local { path: String },
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub name: String,
    pub source: String,
    pub origin: Option<Origin>,
    #[serde(default)]
    pub download_source: Option<DownloadSource>,
    pub baseline: String,
    pub expected: BTreeSet<String>,
}
impl Record {
    pub fn update_origin(&self) -> Option<Origin> {
        self.origin.clone().or_else(|| match self.initial_source() {
            DownloadSource::Remote { origin } => Some(origin),
            _ => None,
        })
    }
    pub fn initial_source(&self) -> DownloadSource {
        self.download_source.clone().unwrap_or_else(|| {
            self.origin
                .clone()
                .map(|origin| DownloadSource::Remote { origin })
                .unwrap_or(DownloadSource::Unknown)
        })
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub clients: BTreeMap<String, String>,
    pub origin: Option<Origin>,
    pub download_source: DownloadSource,
    pub local_modified: bool,
    pub managed: bool,
    pub problem: Option<String>,
    pub drift: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub skills: Vec<Skill>,
    pub warnings: Vec<String>,
    pub pending: bool,
    pub library: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Discovery {
    pub token: String,
    pub repo: String,
    pub reference: String,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryDetails {
    pub name: String,
    pub description: String,
    pub repo: String,
    pub reference: String,
    pub path: String,
    pub content: String,
    pub files: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub kind: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub token: String,
    pub kind: String,
    pub record: Record,
    pub before: Option<Record>,
    pub old_hash: Option<String>,
    pub new_hash: String,
    pub local_modified: bool,
    pub files: Vec<FileChange>,
    pub old_content: String,
    pub new_content: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkStep {
    client: String,
    path: String,
    before: Option<String>,
    enable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Journal {
    token: String,
    plan: Option<Plan>,
    before: Option<Record>,
    after: Record,
    links: Vec<LinkStep>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub token: String,
    pub id: String,
    pub name: String,
    pub created: u64,
    pub previous: Record,
    pub digest: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub id: String,
    pub status: String,
    pub message: String,
    pub local_modified: bool,
}
pub struct Manager {
    home: PathBuf,
    data: PathBuf,
    db: Connection,
}
struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
impl Manager {
    pub fn new(home: &Path, data: &Path) -> Result<Self> {
        fs::create_dir_all(data).map_err(e)?;
        let home = home.canonicalize().map_err(e)?;
        let data = data.canonicalize().map_err(e)?;
        let db = Connection::open(data.join("global.sqlite")).map_err(e)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS skills(id TEXT PRIMARY KEY, record TEXT NOT NULL); CREATE TABLE IF NOT EXISTS admin(key TEXT PRIMARY KEY,value TEXT NOT NULL);",
        )
        .map_err(e)?;
        Ok(Self { home, data, db })
    }
    fn lock(&self) -> Result<Lock> {
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.data.join("lock"))
            .map_err(e)?;
        f.try_lock().map_err(|_| "全局 Skill 正在执行其他操作")?;
        Ok(Lock(f))
    }
    fn ready(&self) -> Result<()> {
        if self.data.join("transaction.json").exists()
            || self.data.join("admin-transaction.json").exists()
        {
            return Err("全局资源有中断操作，请先恢复中断操作".into());
        }
        Ok(())
    }
    fn library(&self) -> PathBuf {
        self.config()
            .ok()
            .filter(|c| !c.library.is_empty())
            .map(|c| PathBuf::from(c.library))
            .unwrap_or_else(|| self.home.join(".cc-switch/skills"))
    }
    fn records(&self) -> Result<BTreeMap<String, Record>> {
        let mut stmt = self.db.prepare("SELECT record FROM skills").map_err(e)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(e)?;
        let mut records = BTreeMap::new();
        for row in rows {
            let record: Record = serde_json::from_str(&row.map_err(e)?).map_err(e)?;
            records.insert(record.id.clone(), record);
        }
        Ok(records)
    }
    fn save(&self, r: &Record) -> Result<()> {
        self.db
            .execute(
                "INSERT OR REPLACE INTO skills(id,record) VALUES(?1,?2)",
                params![r.id, serde_json::to_string(r).map_err(e)?],
            )
            .map_err(e)?;
        Ok(())
    }
    fn restore_record(&self, id: &str, r: &Option<Record>) -> Result<()> {
        if let Some(r) = r {
            self.save(r)
        } else {
            self.db
                .execute("DELETE FROM skills WHERE id=?1", [id])
                .map_err(e)?;
            Ok(())
        }
    }
    fn destination(&self, client: &str, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        let relative = CLIENTS
            .iter()
            .find(|(c, _)| *c == client)
            .ok_or("未知客户端")?
            .1;
        let parent = self
            .config()?
            .clients
            .get(client)
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.join(relative));
        Ok(if parent.exists() {
            parent.canonicalize().map_err(e)?.join(name)
        } else {
            parent.join(name)
        })
    }
    fn cell(&self, source: &Path, client: &str, name: &str) -> String {
        let Ok(dest) = self.destination(client, name) else {
            return "conflict".into();
        };
        if !exists(&dest) {
            return "off".into();
        }
        let linked = fs::symlink_metadata(&dest).is_ok_and(|m| m.file_type().is_symlink());
        if dest.canonicalize().ok() == source.canonicalize().ok() && source.exists() {
            return if linked { "link" } else { "source" }.into();
        }
        if let Ok(cfg) = self.config() {
            if let Some(copy) = cfg.copies.get(&dest.to_string_lossy().to_string()) {
                if copy.source == source.to_string_lossy() {
                    return if digest(&dest).ok().as_ref() == Some(&copy.digest) {
                        "copy"
                    } else {
                        "modified"
                    }
                    .into();
                }
            }
        }
        if linked && !dest.exists() {
            "broken".into()
        } else {
            "conflict".into()
        }
    }
    fn skill_locations(&self, config: &admin::Config) -> Vec<PathBuf> {
        std::iter::once(self.library())
            .chain(std::iter::once(self.home.join(".agents/skills")))
            .chain(std::iter::once(self.home.join(".skill-manager/skills")))
            .chain(std::iter::once(self.home.join(".cc-switch/skills")))
            .chain(std::iter::once(self.home.join(".openclaw/skills")))
            .chain(
                config
                    .clients
                    .iter()
                    .filter(|(client, _)| CLIENTS.iter().any(|(name, _)| name == client))
                    .map(|(_, path)| PathBuf::from(path)),
            )
            .chain(CLIENTS.iter().map(|(_, p)| self.home.join(p)))
            .collect()
    }
    pub fn inventory(&self) -> Result<Inventory> {
        let records = self.records()?;
        let mut sources: BTreeMap<String, (String, String)> = BTreeMap::new();
        let mut warnings = Vec::new();
        let config = self.config()?;
        let locations = self.skill_locations(&config);
        for dir in locations {
            if !dir.exists() {
                continue;
            }
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) => {
                    warnings.push(format!("{}：{e}", dir.display()));
                    continue;
                }
            };
            for entry in entries {
                let entry = entry.map_err(e)?;
                let p = entry.path();
                if config.copies.contains_key(&p.to_string_lossy().to_string()) {
                    continue;
                }
                if !p.join("SKILL.md").is_file() {
                    if fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink())
                        && !p.exists()
                    {
                        warnings.push(format!("断开的软链接：{}", p.display()));
                    }
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if validate_name(&name).is_err() {
                    continue;
                }
                let source = p.canonicalize().map_err(e)?.to_string_lossy().to_string();
                let id = hash(source.as_bytes());
                sources.entry(id).or_insert((name, source));
            }
        }
        for r in records.values() {
            sources.insert(r.id.clone(), (r.name.clone(), r.source.clone()));
        }
        let mut skills = Vec::new();
        for (id, (name, source)) in sources {
            let rec = records.get(&id);
            let path = Path::new(&source);
            let text = self.content(path);
            let fingerprint = digest(path);
            let problem = fingerprint.as_ref().err().cloned();
            let local_modified =
                rec.is_some_and(|r| fingerprint.as_ref().is_ok_and(|h| h != &r.baseline));
            let clients: BTreeMap<_, _> = CLIENTS
                .iter()
                .map(|(c, _)| ((*c).into(), self.cell(path, c, &name)))
                .collect();
            let drift = rec
                .map(|r| {
                    r.expected
                        .iter()
                        .filter(|c| {
                            clients
                                .get(*c)
                                .is_none_or(|s| s != "link" && s != "source" && s != "copy")
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            skills.push(Skill {
                id,
                name,
                source,
                description: frontmatter(&text, "description").unwrap_or_default(),
                clients,
                origin: rec.and_then(|r| r.origin.clone()),
                download_source: rec
                    .map(Record::initial_source)
                    .unwrap_or(DownloadSource::Unknown),
                local_modified,
                managed: rec.is_some(),
                problem,
                drift,
            });
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name).then(a.source.cmp(&b.source)));
        Ok(Inventory {
            skills,
            warnings,
            pending: self.data.join("transaction.json").exists()
                || self.data.join("admin-transaction.json").exists(),
            library: self.library().to_string_lossy().into(),
        })
    }
    fn resolve(&self, id: &str) -> Result<(Skill, Option<Record>)> {
        let skill = self
            .inventory()?
            .skills
            .into_iter()
            .find(|s| s.id == id)
            .ok_or("Skill 已不存在，请刷新")?;
        let record = self.records()?.remove(id);
        Ok((skill, record))
    }
    fn record(&self, id: &str) -> Result<Record> {
        let (s, r) = self.resolve(id)?;
        if let Some(r) = r {
            return Ok(r);
        }
        Ok(Record {
            id: s.id,
            name: s.name,
            source: s.source.clone(),
            origin: None,
            download_source: Some(DownloadSource::Unknown),
            baseline: digest(Path::new(&s.source))?,
            expected: s
                .clients
                .into_iter()
                .filter_map(|(c, status)| (status == "link").then_some(c))
                .collect(),
        })
    }
    fn content(&self, path: &Path) -> String {
        let mut bytes = Vec::new();
        if let Ok(file) = File::open(path.join("SKILL.md")) {
            let _ = file.take(256 * 1024).read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into()
    }
    pub fn details(&self, id: &str) -> Result<String> {
        let (s, _) = self.resolve(id)?;
        Ok(self.content(Path::new(&s.source)))
    }
    pub fn replace_translation(
        &self,
        id: &str,
        expected: &str,
        replacement: &str,
        backups: &Path,
    ) -> Result<crate::llm::Replacement> {
        let _lock = self.lock()?;
        self.ready()?;
        let (skill, _) = self.resolve(id)?;
        let source = Path::new(&skill.source).canonicalize().map_err(e)?;
        crate::llm::replace_file(&source.join("SKILL.md"), expected, replacement, backups)
    }
    pub fn discover(&self, repo: &str, reference: &str) -> Result<Discovery> {
        let _lock = self.lock()?;
        self.ready()?;
        let temp = tempfile::tempdir().map_err(e)?;
        registry::fetch_repo(repo, reference, temp.path())?;
        self.cache_discovery(repo, reference, temp.path())
    }
    fn cache_discovery(&self, repo: &str, reference: &str, source: &Path) -> Result<Discovery> {
        self.cache_discovery_named(
            repo,
            reference,
            source,
            repo.rsplit('/').next().unwrap_or("skill"),
        )
    }
    fn cache_discovery_named(
        &self,
        repo: &str,
        reference: &str,
        source: &Path,
        fallback: &str,
    ) -> Result<Discovery> {
        let candidates = registry::candidates_with_fallback(source, fallback)?;
        if candidates.is_empty() {
            return Err("仓库中未找到包含 SKILL.md 的有效 Skill".into());
        }
        let token = token();
        let dir = self.data.join("discoveries").join(&token);
        fs::create_dir_all(&dir).map_err(e)?;
        copy_tree(source, &dir.join("tree"))?;
        let result = Discovery {
            token,
            repo: repo.into(),
            reference: reference.into(),
            candidates,
        };
        atomic(&dir.join("discovery.json"), &result)?;
        Ok(result)
    }
    /// Inspect downloaded content without preparing an install or touching the library.
    pub fn discovery_details(&self, discovery: &str, path: &str) -> Result<DiscoveryDetails> {
        let _lock = self.lock()?;
        valid_token(discovery)?;
        if path != "." {
            registry::relative(path)?;
        }
        let cache = self.data.join("discoveries").canonicalize().map_err(e)?;
        let dir = cache.join(discovery).canonicalize().map_err(e)?;
        if !dir.starts_with(&cache) {
            return Err("Skill 缓存路径已变化，请重新读取仓库".into());
        }
        let d: Discovery = read(&dir.join("discovery.json"))?;
        let candidate = d
            .candidates
            .iter()
            .find(|c| c.path == path)
            .ok_or("仓库中没有选择的 Skill 路径")?;
        let tree = dir.join("tree").canonicalize().map_err(e)?;
        let source = tree.join(path).canonicalize().map_err(e)?;
        if !tree.starts_with(&dir) || !source.starts_with(&tree) {
            return Err("Skill 路径超出下载的仓库".into());
        }
        let files = safe_tree(&source)?
            .into_keys()
            .filter(|p| !p.ends_with('/'))
            .collect();
        let mut bytes = Vec::new();
        File::open(source.join("SKILL.md"))
            .map_err(e)?
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(e)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("SKILL.md 超过 2 MB，无法在详情中完整显示".into());
        }
        let content = String::from_utf8(bytes).map_err(|_| "SKILL.md 不是有效的 UTF-8 文本")?;
        Ok(DiscoveryDetails {
            name: candidate.name.clone(),
            description: candidate.description.clone(),
            repo: d.repo,
            reference: d.reference,
            path: path.into(),
            content,
            files,
        })
    }
    pub fn prepare_remote(
        &self,
        discovery: &str,
        path: &str,
        existing: Option<&str>,
    ) -> Result<Plan> {
        let _lock = self.lock()?;
        self.ready()?;
        valid_token(discovery)?;
        let dir = self.data.join("discoveries").join(discovery);
        let d: Discovery = read(&dir.join("discovery.json"))?;
        let candidate = d
            .candidates
            .iter()
            .find(|c| c.path == path)
            .ok_or("仓库中没有选择的 Skill 路径")?;
        if path != "." {
            registry::relative(path)?;
        }
        registry::validate_repo(&d.repo, &d.reference)?;
        self.prepare(
            &dir.join("tree").join(path),
            &candidate.name,
            Some(Origin {
                repo: d.repo,
                reference: d.reference,
                path: path.into(),
            }),
            existing,
            "install",
        )
    }
    pub fn prepare_local(&self, path: &Path) -> Result<Plan> {
        let _lock = self.lock()?;
        self.ready()?;
        let source = path.canonicalize().map_err(e)?;
        let name = source.file_name().ok_or("目录没有名称")?.to_string_lossy();
        self.prepare(&source, &name, None, None, "import")
    }
    fn prepare(
        &self,
        payload: &Path,
        name: &str,
        origin: Option<Origin>,
        existing: Option<&str>,
        kind: &str,
    ) -> Result<Plan> {
        validate_name(name)?;
        if !payload.join("SKILL.md").is_file() {
            return Err("选择的目录没有 SKILL.md".into());
        }
        safe_tree(payload)?;
        let before = existing.map(|id| self.record(id)).transpose()?;
        let (source, name) = if let Some(r) = &before {
            let source = PathBuf::from(&r.source);
            self.ensure_shared(&source)?;
            (source, r.name.clone())
        } else {
            fs::create_dir_all(self.library()).map_err(e)?;
            (
                self.library().canonicalize().map_err(e)?.join(name),
                name.to_string(),
            )
        };
        if existing.is_none() && exists(&source) {
            return Err(format!(
                "共享库已存在 {name}，请在已安装列表选择该版本并关联远端更新；不会覆盖同名目录。"
            ));
        }
        let old_hash = if exists(&source) {
            Some(digest(&source)?)
        } else {
            if before.is_some() && kind != "restore" {
                return Err("源目录已丢失，请先恢复备份".into());
            }
            None
        };
        let token = token();
        let dir = self.data.join("plans").join(&token);
        fs::create_dir_all(&dir).map_err(e)?;
        copy_tree(payload, &dir.join("payload"))?;
        let new_hash = digest(&dir.join("payload"))?;
        let old_files = if source.exists() {
            safe_tree(&source)?
        } else {
            BTreeMap::new()
        };
        let new_files = safe_tree(&dir.join("payload"))?;
        let keys: BTreeSet<_> = old_files.keys().chain(new_files.keys()).cloned().collect();
        let files = keys
            .into_iter()
            .filter(|k| old_files.get(k) != new_files.get(k))
            .map(|path| FileChange {
                kind: if !old_files.contains_key(&path) {
                    "added"
                } else if !new_files.contains_key(&path) {
                    "removed"
                } else {
                    "modified"
                }
                .into(),
                path,
            })
            .collect();
        let known_records = self.records()?;
        let before_db = existing.and_then(|id| known_records.get(id).cloned());
        let record = Record {
            id: hash(source.to_string_lossy().as_bytes()),
            name,
            source: source.to_string_lossy().into(),
            download_source: Some(before.as_ref().map(Record::initial_source).unwrap_or_else(
                || {
                    origin
                        .clone()
                        .map(|origin| DownloadSource::Remote { origin })
                        .unwrap_or_else(|| DownloadSource::Local {
                            path: payload.to_string_lossy().into(),
                        })
                },
            )),
            origin,
            baseline: new_hash.clone(),
            expected: before
                .as_ref()
                .map(|r| r.expected.clone())
                .unwrap_or_default(),
        };
        let plan = Plan {
            token,
            kind: if before.is_some() && kind != "restore" {
                "update".into()
            } else {
                kind.into()
            },
            record,
            before: before_db,
            old_hash: old_hash.clone(),
            new_hash,
            local_modified: before
                .as_ref()
                .is_some_and(|r| r.origin.is_none() || old_hash.as_ref() != Some(&r.baseline)),
            files,
            old_content: self.content(&source),
            new_content: self.content(&dir.join("payload")),
        };
        atomic(&dir.join("plan.json"), &plan)?;
        Ok(plan)
    }
    fn ensure_shared(&self, source: &Path) -> Result<()> {
        validate_name(
            source
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("源路径无效")?,
        )?;
        let parent = source.parent().ok_or("源路径无效")?;
        for base in [
            self.library(),
            self.home.join(".agents/skills"),
            self.home.join(".cc-switch/skills"),
            self.home.join(".skill-manager/skills"),
        ] {
            if base.canonicalize().is_ok_and(|p| p == parent) {
                return Ok(());
            }
        }
        Err("这个 Skill 的源位于客户端或外部目录。请先导入共享库，再为共享版本关联远端；不会直接升级外部项目文件。".into())
    }
    pub fn prepare_update(&self, id: &str) -> Result<Plan> {
        self.prepare_update_with(id, registry::resolve_update_origin, registry::fetch_repo)
    }
    fn prepare_update_with(
        &self,
        id: &str,
        resolve: impl Fn(&Origin) -> Result<Origin>,
        fetch: impl Fn(&str, &str, &Path) -> Result<()>,
    ) -> Result<Plan> {
        let _lock = self.lock()?;
        self.ready()?;
        let record = self.record(id)?;
        let origin = record
            .update_origin()
            .ok_or("该 Skill 尚未关联远端。请搜索或输入 GitHub 仓库，选择精确 Skill 路径。")?;
        let temp = tempfile::tempdir().map_err(e)?;
        let origin = resolve(&origin)?;
        fetch(&origin.repo, &origin.reference, temp.path())?;
        if origin.path != "." {
            registry::relative(&origin.path)?;
        }
        self.prepare(
            &temp.path().join(&origin.path),
            &record.name,
            Some(origin),
            Some(id),
            "update",
        )
    }
    pub fn apply(&self, token: &str, allow_local_changes: bool) -> Result<()> {
        self.apply_content_admin(token, allow_local_changes)
    }
    #[doc(hidden)]
    pub fn apply_with_failure(
        &self,
        token: &str,
        allow_local_changes: bool,
        fail_at: Option<usize>,
    ) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        valid_token(token)?;
        let plan: Plan = read(&self.data.join("plans").join(token).join("plan.json"))?;
        if plan.token != token {
            return Err("操作编号不一致".into());
        }
        self.ensure_shared(Path::new(&plan.record.source))?;
        if plan.record.id != hash(plan.record.source.as_bytes()) {
            return Err("资源身份不一致".into());
        }
        if self.records()?.get(&plan.record.id) != plan.before.as_ref() {
            return Err("预览后 Skill 管理状态已变化，请重新预览".into());
        }
        let source = PathBuf::from(&plan.record.source);
        let payload = self.data.join("plans").join(token).join("payload");
        if digest(&payload)? != plan.new_hash {
            return Err("待安装内容已变化，请重新预览".into());
        }
        if plan.old_hash.is_some() {
            if fs::symlink_metadata(&source)
                .map_err(e)?
                .file_type()
                .is_symlink()
                || Some(digest(&source)?) != plan.old_hash
            {
                return Err("预览后本地内容发生变化，请重新预览".into());
            }
        } else if exists(&source) {
            return Err("安装目标已出现同名内容，不会覆盖".into());
        }
        if plan.local_modified && !allow_local_changes {
            return Err("检测到本地修改，请确认备份后升级".into());
        }
        let journal = Journal {
            token: token.into(),
            plan: Some(plan.clone()),
            before: plan.before.clone(),
            after: plan.record.clone(),
            links: Vec::new(),
        };
        let backup_dir = self.data.join("backups").join(token);
        fs::create_dir_all(&backup_dir).map_err(e)?;
        atomic(&self.data.join("transaction.json"), &journal)?;
        let operation: Result<()> = (|| {
            if plan.old_hash.is_some() {
                rename(&source, &backup_dir.join("content"))?;
                if Some(digest(&backup_dir.join("content"))?) != plan.old_hash {
                    return Err("源目录在升级期间发生变化，已保留备份".into());
                }
            }
            if fail_at == Some(1) {
                return Err("注入的升级故障".into());
            }
            rename(&payload, &source)?;
            if digest(&source)? != plan.new_hash {
                return Err("新版本在安装期间发生变化，已保留双方内容".into());
            }
            if fail_at == Some(2) {
                return Err("注入的数据库提交故障".into());
            }
            self.save(&plan.record)?;
            if let Some(old) = &plan.old_hash {
                let previous = plan.before.clone().unwrap_or(Record {
                    id: plan.record.id.clone(),
                    name: plan.record.name.clone(),
                    source: plan.record.source.clone(),
                    origin: None,
                    download_source: Some(DownloadSource::Unknown),
                    baseline: old.clone(),
                    expected: plan.record.expected.clone(),
                });
                atomic(
                    &backup_dir.join("backup.json"),
                    &Backup {
                        token: token.into(),
                        id: plan.record.id.clone(),
                        name: plan.record.name.clone(),
                        created: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_err(e)?
                            .as_secs(),
                        previous,
                        digest: old.clone(),
                    },
                )?;
            }
            fs::remove_file(self.data.join("transaction.json")).map_err(e)?;
            Ok(())
        })();
        if let Err(error) = operation {
            return match self.rollback(&journal) {
                Ok(()) => Err(format!("操作失败并已回滚：{error}")),
                Err(recovery) => Err(format!("操作失败：{error}；需要恢复：{recovery}")),
            };
        }
        Ok(())
    }
    pub fn toggle(&self, id: &str, clients: Vec<String>, enable: bool) -> Result<()> {
        let p = self.prepare_sync(vec![id.into()], clients, enable, None)?;
        self.apply_admin(&p.token)
    }
    #[allow(dead_code)]
    fn toggle_legacy(&self, id: &str, clients: Vec<String>, enable: bool) -> Result<()> {
        let _lock = self.lock()?;
        self.ready()?;
        let mut record = self.record(id)?;
        let before = self.records()?.remove(id);
        let source = PathBuf::from(&record.source);
        if !source.join("SKILL.md").is_file() {
            return Err("Skill 源目录不存在".into());
        }
        let mut paths = BTreeSet::new();
        let mut steps = Vec::new();
        for client in clients {
            let dest = self.destination(&client, &record.name)?;
            let status = self.cell(&source, &client, &record.name);
            if status == "source" {
                if !enable {
                    return Err(format!(
                        "{client} 当前目录就是源文件，不能移除。可先导入共享库。"
                    ));
                }
                continue;
            }
            if !["off", "link"].contains(&status.as_str()) {
                return Err(format!(
                    "{client} 中存在同名目录或其他链接，已停止整个批次，不会覆盖。"
                ));
            }
            if enable {
                record.expected.insert(client.clone());
            } else {
                record.expected.remove(&client);
            }
            if (status == "link") == enable || !paths.insert(dest.clone()) {
                continue;
            }
            steps.push(LinkStep {
                client,
                path: dest.to_string_lossy().into(),
                before: if exists(&dest) {
                    Some(fs::read_link(&dest).map_err(e)?.to_string_lossy().into())
                } else {
                    None
                },
                enable,
            });
        }
        let token = token();
        let dir = self.data.join("backups").join(&token);
        fs::create_dir_all(&dir).map_err(e)?;
        let journal = Journal {
            token,
            plan: None,
            before,
            after: record.clone(),
            links: steps,
        };
        atomic(&self.data.join("transaction.json"), &journal)?;
        let operation: Result<()> = (|| {
            for (i, step) in journal.links.iter().enumerate() {
                let dest = Path::new(&step.path);
                if self.destination(&step.client, &record.name)? != dest {
                    return Err("客户端目录在操作期间发生变化".into());
                }
                fs::create_dir_all(dest.parent().unwrap()).map_err(e)?;
                if step.enable {
                    create_link(&source, dest)?;
                } else {
                    if fs::read_link(dest).map_err(e)?.to_string_lossy()
                        != step.before.as_deref().unwrap()
                    {
                        return Err("链接在操作期间发生变化".into());
                    }
                    rename(dest, &dir.join(format!("link-{i}")))?;
                    if fs::read_link(dir.join(format!("link-{i}")))
                        .map_err(e)?
                        .to_string_lossy()
                        != step.before.as_deref().unwrap()
                    {
                        return Err("移走的链接发生变化，已保留备份".into());
                    }
                }
            }
            self.save(&record)?;
            fs::remove_file(self.data.join("transaction.json")).map_err(e)?;
            Ok(())
        })();
        if let Err(error) = operation {
            return match self.rollback(&journal) {
                Ok(()) => Err(format!("链接操作失败并已回滚：{error}")),
                Err(r) => Err(format!("链接操作失败：{error}；需要恢复：{r}")),
            };
        }
        Ok(())
    }
    fn rollback(&self, j: &Journal) -> Result<()> {
        valid_token(&j.token)?;
        let backup = self.data.join("backups").join(&j.token);
        if let Some(p) = &j.plan {
            self.ensure_shared(Path::new(&p.record.source))?;
            let source = Path::new(&p.record.source);
            let payload = self.data.join("plans").join(&j.token).join("payload");
            let old = backup.join("content");
            if !exists(&payload) && exists(source) {
                if digest(source)? != p.new_hash {
                    return Err("新版本在恢复前被修改，未覆盖，需手动保留双方".into());
                }
                rename(source, &payload)?;
            }
            if exists(&old) {
                if exists(source) || Some(digest(&old)?) != p.old_hash {
                    return Err("原版本备份或目标发生冲突，未覆盖".into());
                }
                rename(&old, source)?;
            }
            if p.old_hash.is_some() && Some(digest(source)?) != p.old_hash {
                return Err("原版本未完整恢复".into());
            }
        }
        for (i, step) in j.links.iter().enumerate().rev() {
            let dest = Path::new(&step.path);
            if self.destination(&step.client, &j.after.name)? != dest {
                return Err("客户端路径已变化，请手动恢复链接".into());
            }
            let old = backup.join(format!("link-{i}"));
            if step.enable {
                if exists(dest) {
                    if fs::read_link(dest).map_err(e)? != Path::new(&j.after.source) {
                        return Err("目标已被外部替换，未移除".into());
                    }
                    rename(dest, &backup.join(format!("rollback-{i}")))?;
                }
            } else if exists(&old) {
                if exists(dest)
                    || fs::read_link(&old).map_err(e)?.to_string_lossy()
                        != step.before.as_deref().unwrap_or("")
                {
                    return Err("链接恢复遇到同名内容".into());
                }
                rename(&old, dest)?;
            } else if fs::read_link(dest).map_err(e)?.to_string_lossy()
                != step.before.as_deref().unwrap_or("")
            {
                return Err("原链接已被外部替换，未完成恢复".into());
            }
        }
        self.restore_record(&j.after.id, &j.before)?;
        fs::remove_file(self.data.join("transaction.json")).map_err(e)?;
        Ok(())
    }
    pub fn recover(&self) -> Result<()> {
        self.recover_admin()?;
        let _lock = self.lock()?;
        let path = self.data.join("transaction.json");
        if !path.exists() {
            return Ok(());
        }
        self.rollback(&read(&path)?)
    }
    pub fn backups(&self, id: &str) -> Result<Vec<Backup>> {
        let mut list = Vec::new();
        let dir = self.data.join("backups");
        if !dir.exists() {
            return Ok(list);
        }
        for entry in fs::read_dir(dir).map_err(e)? {
            let dir = entry.map_err(e)?.path();
            if dir.join("backup.json").exists() && dir.join("content").exists() {
                let b: Backup = read(&dir.join("backup.json"))?;
                if b.id == id || self.config()?.aliases.get(&b.id).is_some_and(|x| x == id) {
                    list.push(b);
                }
            }
        }
        list.sort_by(|a, b| b.created.cmp(&a.created));
        Ok(list)
    }
    pub fn prepare_restore(&self, id: &str, token: &str) -> Result<Plan> {
        let _lock = self.lock()?;
        self.ready()?;
        valid_token(token)?;
        let dir = self.data.join("backups").join(token);
        let b: Backup = read(&dir.join("backup.json"))?;
        if (b.id != id && !self.config()?.aliases.get(&b.id).is_some_and(|x| x == id))
            || digest(&dir.join("content"))? != b.digest
        {
            return Err("备份身份或内容校验失败".into());
        }
        self.prepare(
            &dir.join("content"),
            &b.name,
            b.previous.origin,
            Some(id),
            "restore",
        )
    }
    pub fn check_updates(&self) -> Result<Vec<Update>> {
        self.check_updates_with(registry::resolve_update_origin, registry::fetch_repo)
    }
    fn check_updates_with(
        &self,
        resolve: impl Fn(&Origin) -> Result<Origin>,
        fetch: impl Fn(&str, &str, &Path) -> Result<()>,
    ) -> Result<Vec<Update>> {
        let _lock = self.lock()?;
        self.ready()?;
        let records = self.records()?;
        let mut repos: BTreeMap<(String, String), std::result::Result<tempfile::TempDir, String>> =
            BTreeMap::new();
        let mut updates = Vec::new();
        let mut resolved: BTreeMap<(String, String), Result<String>> = BTreeMap::new();
        for r in records.values() {
            let Some(mut origin) = r.update_origin() else {
                continue;
            };
            let original_reference = origin.reference.clone();
            let resolved_reference = resolved
                .entry((origin.repo.clone(), origin.reference.clone()))
                .or_insert_with(|| resolve(&origin).map(|o| o.reference));
            match resolved_reference {
                Ok(reference) => origin.reference = reference.clone(),
                Err(error) => {
                    updates.push(Update {
                        id: r.id.clone(),
                        status: "error".into(),
                        message: error.clone(),
                        local_modified: digest(Path::new(&r.source)).is_ok_and(|h| h != r.baseline),
                    });
                    continue;
                }
            }
            let key = (origin.repo.clone(), origin.reference.clone());
            if !repos.contains_key(&key) {
                let result = (|| {
                    let t = tempfile::tempdir().map_err(e)?;
                    fetch(&origin.repo, &origin.reference, t.path())?;
                    Ok(t)
                })();
                repos.insert(key.clone(), result);
            }
            let local = digest(Path::new(&r.source));
            let modified = local.as_ref().is_ok_and(|h| h != &r.baseline);
            let remote = match &repos[&key] {
                Ok(t) => {
                    if origin.path != "." {
                        registry::relative(&origin.path)?;
                    }
                    digest(&t.path().join(&origin.path))
                }
                Err(error) => Err(error.clone()),
            };
            let (status, message) = match (local, remote) {
                (Err(e), _) | (_, Err(e)) => ("error", e),
                (Ok(a), Ok(b)) if a == b => ("latest", "已是远端版本".into()),
                (Ok(_), Ok(b)) if b == r.baseline => ("local", "远端未变化，本地有修改".into()),
                _ if original_reference != origin.reference => (
                    "available",
                    format!(
                        "发现新版本 {} → {}，可预览升级",
                        original_reference, origin.reference
                    ),
                ),
                _ => ("available", "与远端存在差异，可预览升级".into()),
            };
            updates.push(Update {
                id: r.id.clone(),
                status: status.into(),
                message,
                local_modified: modified,
            });
        }
        Ok(updates)
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;

    #[test]
    fn download_source_update_checks_previews_applies_and_restores() {
        use std::cell::Cell;
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir(&home).unwrap();
        let m = Manager::new(&home, &temp.path().join("data")).unwrap();
        let input = temp.path().join("demo");
        fs::create_dir(&input).unwrap();
        fs::write(
            input.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\nold",
        )
        .unwrap();
        let plan = m.prepare_local(&input).unwrap();
        m.apply(&plan.token, false).unwrap();
        let mut record = m.record(&plan.record.id).unwrap();
        let origin = Origin {
            repo: "example/repo".into(),
            reference: "v1.0.60".into(),
            path: "skills/demo".into(),
        };
        record.download_source = Some(DownloadSource::Remote {
            origin: origin.clone(),
        });
        m.save(&record).unwrap();
        let resolve = |o: &Origin| {
            let mut n = o.clone();
            n.reference = "v1.0.61".into();
            Ok(n)
        };
        let calls = Cell::new(0);
        let fetch = |repo: &str, reference: &str, dest: &Path| {
            assert_eq!(repo, "example/repo");
            assert_eq!(reference, "v1.0.61");
            calls.set(calls.get() + 1);
            fs::create_dir_all(dest.join("skills/demo")).unwrap();
            fs::write(
                dest.join("skills/demo/SKILL.md"),
                "---\nname: demo\ndescription: Demo\n---\nnew",
            )
            .unwrap();
            Ok(())
        };
        let updates = m.check_updates_with(resolve, fetch).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].status, "available");
        assert!(updates[0].message.contains("v1.0.60 → v1.0.61"));
        let plan = m.prepare_update_with(&record.id, resolve, fetch).unwrap();
        assert_eq!(plan.record.origin.as_ref().unwrap().reference, "v1.0.61");
        assert!(plan.new_content.contains("new"));
        m.apply(&plan.token, true).unwrap();
        let updated = m.record(&record.id).unwrap();
        assert_eq!(updated.download_source, record.download_source);
        assert_eq!(updated.origin.unwrap().reference, "v1.0.61");
        assert_eq!(
            m.check_updates_with(resolve, fetch).unwrap()[0].status,
            "latest"
        );
        let restore = m.prepare_restore(&record.id, &plan.token).unwrap();
        m.apply(&restore.token, true).unwrap();
        assert!(
            fs::read_to_string(Path::new(&record.source).join("SKILL.md"))
                .unwrap()
                .ends_with("old")
        );
        let errors = m
            .check_updates_with(|_| Err("release service unavailable".into()), fetch)
            .unwrap();
        assert_eq!(errors[0].status, "error");
        assert!(errors[0].message.contains("release service"));
        // Explicit upgrade sources win over original download locations.
        let mut explicit = record.clone();
        explicit.origin = Some(Origin {
            repo: "other/repo".into(),
            ..origin
        });
        assert_eq!(explicit.update_origin().unwrap().repo, "other/repo");
    }

    #[test]
    fn download_source_is_not_silently_skipped() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir(&home).unwrap();
        let m = Manager::new(&home, &temp.path().join("data")).unwrap();
        m.save(&Record {
            id: "demo".into(),
            name: "demo".into(),
            source: home.join("missing").to_string_lossy().into(),
            origin: None,
            download_source: Some(DownloadSource::Remote {
                origin: Origin {
                    repo: "invalid".into(),
                    reference: "main".into(),
                    path: "demo".into(),
                },
            }),
            baseline: String::new(),
            expected: BTreeSet::new(),
        })
        .unwrap();
        let updates = m.check_updates().unwrap();
        assert_eq!(
            updates.len(),
            1,
            "download-source skills must participate, including errors"
        );
        assert_eq!(updates[0].status, "error");
    }
}
