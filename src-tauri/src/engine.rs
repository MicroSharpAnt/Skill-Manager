//! Project files remain the source of truth. A write-ahead journal precedes every
//! rename; the Git hook fails closed until all parked resources are restored.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};

pub type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
pub fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(err)?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
fn git_text(root: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&git(root, args)?).trim().to_owned())
}
fn sync_dir(p: &Path) -> Result<()> {
    File::open(p).and_then(|f| f.sync_all()).map_err(err)
}
fn atomic(p: &Path, bytes: &[u8]) -> Result<()> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(err)?
        .as_nanos();
    let temp = p.with_extension(format!("{}-{nonce}.tmp", std::process::id()));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(err)?;
    f.write_all(bytes).and_then(|_| f.sync_all()).map_err(err)?;
    fs::rename(&temp, p).map_err(err)?;
    sync_dir(p.parent().unwrap())
}
fn json_write<T: Serialize>(p: &Path, value: &T) -> Result<()> {
    atomic(p, &serde_json::to_vec_pretty(value).map_err(err)?)
}
fn read_json<T: for<'a> Deserialize<'a>>(p: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(p).map_err(err)?)
        .map_err(|e| format!("恢复记录无法读取（{}）：{e}。请保留暂存目录。", p.display()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parked {
    pub path: String,
    pub digest: String,
    pub head: String,
    pub index: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    version: u32,
    root: String,
    git_dir: String,
    pub parked: BTreeMap<String, Parked>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Move {
    path: String,
    enable: bool,
    digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Journal {
    before: Manifest,
    moves: Vec<Move>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub path: String,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub provider: String,
    pub enabled: bool,
    pub status: String,
    pub digest: String,
    pub updated_at: Option<u64>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub root: String,
    pub branch: String,
    pub vault: String,
    pub hook: String,
    pub resources: Vec<Resource>,
    pub pending: bool,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    pub enable: bool,
}

pub struct Repo {
    pub root: PathBuf,
    pub vault: PathBuf,
    git_dir: PathBuf,
}
struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
impl Repo {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize().map_err(err)?;
        let root = PathBuf::from(git_text(&path, &["rev-parse", "--show-toplevel"])?)
            .canonicalize()
            .map_err(err)?;
        let git_dir = PathBuf::from(git_text(&root, &["rev-parse", "--absolute-git-dir"])?)
            .canonicalize()
            .map_err(err)?;
        let vault = git_dir.join("skill-manager");
        if exists(&vault)
            && fs::symlink_metadata(&vault)
                .map_err(err)?
                .file_type()
                .is_symlink()
        {
            return Err("暂存目录不能是软链接".into());
        }
        Ok(Self {
            root,
            vault,
            git_dir,
        })
    }
    fn blank(&self) -> Manifest {
        Manifest {
            version: 1,
            root: self.root.to_string_lossy().into(),
            git_dir: self.git_dir.to_string_lossy().into(),
            parked: BTreeMap::new(),
        }
    }
    fn validate(&self, m: &Manifest) -> Result<()> {
        if m.version != 1
            || m.root != self.root.to_string_lossy()
            || m.git_dir != self.git_dir.to_string_lossy()
        {
            return Err("项目路径或 worktree 身份发生变化，请先检查原暂存目录并恢复文件".into());
        }
        for (k, v) in &m.parked {
            validate_path(k)?;
            if k != &v.path {
                return Err("暂存清单的资源身份不一致".into());
            }
        }
        Ok(())
    }
    fn manifest(&self) -> Result<Manifest> {
        if !exists(&self.vault) {
            return Ok(self.blank());
        }
        let m: Manifest = read_json(&self.vault.join("state.json"))?;
        self.validate(&m)?;
        Ok(m)
    }
    fn lock(&self) -> Result<Lock> {
        fs::create_dir_all(&self.vault).map_err(err)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.vault.join("lock"))
            .map_err(err)?;
        file.try_lock()
            .map_err(|_| "另一个操作正在管理此项目，请稍后重试".to_string())?;
        Ok(Lock(file))
    }
    fn persist(&self, m: &Manifest) -> Result<()> {
        json_write(&self.vault.join("state.json"), m)
    }
    fn block(&self) -> Result<()> {
        let p = self.vault.join("ready.hash");
        if exists(&p) {
            fs::remove_file(p).map_err(err)?;
            sync_dir(&self.vault)?;
        }
        Ok(())
    }
    fn certify(&self, m: &Manifest) -> Result<()> {
        self.block()?;
        if m.parked.is_empty() && !exists(&self.vault.join("transaction.json")) {
            let backups = self.vault.join("resources");
            if backups.exists() && fs::read_dir(&backups).map_err(err)?.next().is_some() {
                return Err(
                    "清单为空但暂存目录仍有文件，不能解除提交保护。请保留备份并检查 RECOVERY.md。"
                        .into(),
                );
            }
            let digest = git(
                &self.root,
                &[
                    "hash-object",
                    self.vault
                        .join("state.json")
                        .to_str()
                        .ok_or("路径不是 UTF-8")?,
                ],
            )?;
            atomic(&self.vault.join("ready.hash"), &digest)?;
        }
        Ok(())
    }
    fn backup(&self, p: &str) -> PathBuf {
        self.vault.join("resources").join(hash(p.as_bytes()))
    }
    fn head(&self) -> String {
        git_text(&self.root, &["rev-parse", "HEAD"]).unwrap_or_else(|_| "unborn".into())
    }
    fn index(&self, path: &str) -> Result<String> {
        Ok(hash(&git(
            &self.root,
            &[
                "--literal-pathspecs",
                "ls-files",
                "--stage",
                "-z",
                "--",
                path,
            ],
        )?))
    }
    fn hooks_dir(&self) -> Result<PathBuf> {
        if !git_text(&self.root, &["config", "--get", "core.hooksPath"])
            .unwrap_or_default()
            .is_empty()
        {
            return Err(
                "项目使用了自定义 core.hooksPath；首版不接管它，请先手动整合提交检查".into(),
            );
        }
        Ok(PathBuf::from(git_text(
            &self.root,
            &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
        )?))
    }
    pub fn hook_status(&self) -> String {
        let Ok(dir) = self.hooks_dir() else {
            return "custom".into();
        };
        match fs::read(dir.join("pre-commit")) {
            Ok(b) if b == HOOK.as_bytes() && executable(&dir.join("pre-commit")) => {
                "protected".into()
            }
            Ok(_) => "existing".into(),
            Err(_) => "missing".into(),
        }
    }
    pub fn install_hook(&self, chain_existing: bool) -> Result<()> {
        let dir = self.hooks_dir()?;
        no_symlink_ancestors(&dir)?;
        let _lock = self.lock()?;
        let state = self.vault.join("state.json");
        // Existing vaults with recovery material must never be reinitialized.
        if !exists(&state) {
            let leftovers = fs::read_dir(&self.vault)
                .map_err(err)?
                .filter_map(|x| x.ok())
                .any(|x| x.file_name() != "lock");
            if leftovers {
                return Err("暂存清单缺失但目录仍含恢复资料，请先恢复，不能重新初始化".into());
            }
            self.persist(&self.blank())?;
        }
        let m = self.manifest()?;
        fs::create_dir_all(&dir).map_err(err)?;
        let hook = dir.join("pre-commit");
        if self.hook_status() != "protected" && exists(&hook) {
            if !chain_existing {
                return Err("已有 pre-commit，需要明确确认后备份并串联".into());
            }
            if exists(&dir.join("pre-commit.skill-manager.previous")) {
                return Err("已有 Hook 备份，请先检查，不能覆盖".into());
            }
            if fs::symlink_metadata(&hook)
                .map_err(err)?
                .file_type()
                .is_symlink()
            {
                return Err("已有 Hook 为软链接，请手动整合".into());
            }
            fs::rename(&hook, dir.join("pre-commit.skill-manager.previous")).map_err(err)?;
        }
        atomic(&hook, HOOK.as_bytes())?;
        set_executable(&hook)?;
        fs::write(self.vault.join("RECOVERY.md"), RECOVERY).map_err(err)?;
        self.certify(&m)
    }
    pub fn scan(&self) -> Result<Snapshot> {
        let m = self.manifest()?;
        let mut paths = BTreeSet::new();
        let mut warnings = Vec::new();
        for (base, kind, _) in PROVIDERS {
            let dir = self.root.join(base);
            if !dir.exists() {
                continue;
            }
            if let Err(e) = no_symlink_ancestors(&dir) {
                warnings.push(format!(
                    "{base} 是链接入口，未重复纳管；请管理其源目录。{e}"
                ));
                continue;
            }
            for entry in fs::read_dir(dir).map_err(err)? {
                let entry = entry.map_err(err)?;
                let p = entry.path();
                if (*kind == "skill" && p.join("SKILL.md").exists())
                    || (*kind == "rule" && p.extension().is_some_and(|v| v == "mdc"))
                {
                    if let Some(name) = entry.file_name().to_str() {
                        paths.insert(format!("{base}/{name}"));
                    }
                }
            }
        }
        paths.extend(m.parked.keys().cloned());
        let mut resources = Vec::new();
        for path in paths {
            let parked = m.parked.get(&path);
            let enabled = parked.is_none();
            let location = if enabled {
                self.root.join(&path)
            } else {
                self.backup(&path)
            };
            let info = validate_path(&path)?;
            let digest = tree_digest(&location);
            let status = if let Some(p) = parked {
                if exists(&self.root.join(&path)) {
                    "conflict"
                } else if digest.as_ref().is_err() || digest.as_ref().is_ok_and(|d| d != &p.digest)
                {
                    "recovery"
                } else if p.head != self.head() || p.index != self.index(&path)? {
                    "drift"
                } else {
                    "disabled"
                }
            } else if digest.is_err() {
                "unsupported"
            } else {
                "enabled"
            };
            let content_path = if info.0 == "skill" {
                location.join("SKILL.md")
            } else {
                location.clone()
            };
            let content = read_limited(&content_path).unwrap_or_default();
            let name = frontmatter(&content, "name").unwrap_or_else(|| {
                Path::new(&path)
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into()
            });
            let description = frontmatter(&content, "description").unwrap_or_default();
            resources.push(Resource {
                path,
                name,
                description,
                kind: info.0.into(),
                provider: info.1.into(),
                enabled,
                status: status.into(),
                digest: digest.unwrap_or_default(),
                updated_at: resource_updated_at(&location),
            });
        }
        for p in ["AGENTS.md", "CLAUDE.md", ".cursorrules"] {
            if self.root.join(p).exists() {
                warnings.push(format!("检测到 {p}，它独立生效，本页开关不会修改该文件。"));
            }
        }
        Ok(Snapshot {
            root: self.root.to_string_lossy().into(),
            branch: git_text(&self.root, &["branch", "--show-current"]).unwrap_or_default(),
            vault: self.vault.to_string_lossy().into(),
            hook: self.hook_status(),
            pending: exists(&self.vault.join("transaction.json")),
            resources,
            warnings,
        })
    }
    pub fn content(&self, path: &str) -> Result<String> {
        let (kind, _) = validate_path(path)?;
        let m = self.manifest()?;
        let p = if m.parked.contains_key(path) {
            self.backup(path)
        } else {
            self.root.join(path)
        };
        no_symlink_ancestors(&p)?;
        read_limited(&if kind == "skill" {
            p.join("SKILL.md")
        } else {
            p
        })
    }
    pub fn apply(&self, changes: Vec<Change>) -> Result<()> {
        self.apply_inner(changes, None)
    }
    pub fn replace_translation(
        &self,
        path: &str,
        expected: &str,
        replacement: &str,
        backups: &Path,
    ) -> Result<crate::llm::Replacement> {
        let (kind, _) = validate_path(path)?;
        // Do not initialize a project vault merely to edit a document.
        let _lock = if self.vault.exists() {
            Some(self.lock()?)
        } else {
            None
        };
        let manifest = self.manifest()?;
        if self.vault.join("transaction.json").exists() || manifest.parked.contains_key(path) {
            return Err("请先恢复并启用此资源，再覆盖译文".into());
        }
        let location = self.root.join(path);
        no_symlink_ancestors(&location)?;
        tree_digest(&location)?;
        let file = if kind == "skill" {
            location.join("SKILL.md")
        } else {
            location
        };
        crate::llm::replace_file(&file, expected, replacement, backups)
    }
    /// Fault injection stays in the core so integration tests exercise real journals.
    #[doc(hidden)]
    pub fn apply_inner(&self, changes: Vec<Change>, fail_after: Option<usize>) -> Result<()> {
        if self.hook_status() != "protected" {
            return Err("请先启用 Git 提交保护，再开关项目资源".into());
        }
        let _lock = self.lock()?;
        let before = self.manifest()?;
        if exists(&self.vault.join("transaction.json")) {
            return Err("有未完成的操作，请在恢复中心执行恢复".into());
        }
        let mut seen = BTreeSet::new();
        let mut moves = Vec::new();
        let mut after = before.clone();
        for c in changes {
            validate_path(&c.path)?;
            if !seen.insert(c.path.clone()) {
                return Err("批量操作包含重复资源".into());
            }
            let currently_enabled = !before.parked.contains_key(&c.path);
            if c.enable == currently_enabled {
                continue;
            }
            let active = self.root.join(&c.path);
            no_symlink_ancestors(active.parent().unwrap())?;
            let parked = self.backup(&c.path);
            let (src, dst) = if c.enable {
                (&parked, &active)
            } else {
                (&active, &parked)
            };
            if exists(dst) {
                return Err(format!(
                    "{} 的目标位置已有内容，已停止整个批次，不会覆盖",
                    c.path
                ));
            }
            let digest = tree_digest(src)?;
            if c.enable {
                let old = &before.parked[&c.path];
                if old.digest != digest {
                    return Err(format!("{} 的暂存内容已变化，请保留副本并手动恢复", c.path));
                }
                if old.head != self.head() || old.index != self.index(&c.path)? {
                    return Err(format!("{} 关闭后分支或暂存区发生变化。请先切回原提交，并撤销该资源的暂存变化，再恢复。文件备份仍安全保留。",c.path));
                }
                after.parked.remove(&c.path);
            } else {
                if git(
                    &self.root,
                    &["--literal-pathspecs", "ls-files", "-u", "--", &c.path],
                )?
                .len()
                    > 0
                {
                    return Err(format!("{} 存在 Git 合并冲突", c.path));
                }
                let resource_root = if active.is_dir() {
                    active.as_path()
                } else {
                    active.parent().unwrap()
                };
                if git_text(resource_root, &["rev-parse", "--show-toplevel"])?
                    != self.root.to_string_lossy()
                {
                    return Err("嵌套仓库必须单独添加为项目".into());
                }
                if validate_path(&c.path)?.0 == "skill" && !active.join("SKILL.md").is_file() {
                    return Err("Skill 目录缺少 SKILL.md，请重新扫描".into());
                }
                after.parked.insert(
                    c.path.clone(),
                    Parked {
                        path: c.path.clone(),
                        digest: digest.clone(),
                        head: self.head(),
                        index: self.index(&c.path)?,
                    },
                );
            }
            moves.push(Move {
                path: c.path,
                enable: c.enable,
                digest,
            });
        }
        if moves.is_empty() {
            return Ok(());
        }
        fs::create_dir_all(self.vault.join("resources")).map_err(err)?;
        no_symlink_ancestors(&self.vault.join("resources"))?;
        let journal = Journal {
            before: before.clone(),
            moves,
        };
        self.block()?;
        json_write(&self.vault.join("transaction.json"), &journal)?;
        let result = (|| {
            for (i, m) in journal.moves.iter().enumerate() {
                if fail_after == Some(i) {
                    return Err("测试注入的文件操作故障".into());
                }
                let (src, dst) = self.move_paths(m);
                if exists(&dst) || tree_digest(&src)? != m.digest {
                    return Err(format!("{} 操作前内容发生变化", m.path));
                }
                fs::create_dir_all(dst.parent().unwrap()).map_err(err)?;
                rename_exclusive(&src, &dst)?;
            }
            self.persist(&after)?;
            fs::remove_file(self.vault.join("transaction.json")).map_err(err)?;
            self.certify(&after)
        })();
        if let Err(e) = result {
            if !exists(&self.vault.join("transaction.json")) {
                return Err(format!(
                    "文件已完成操作，但提交保护仍锁定，请检查恢复中心：{e}"
                ));
            }
            return match self.rollback(&journal) {
                Ok(()) => Err(format!("操作失败，已完整回滚：{e}")),
                Err(r) => Err(format!("操作失败：{e}；需要恢复：{r}")),
            };
        }
        Ok(())
    }
    fn move_paths(&self, m: &Move) -> (PathBuf, PathBuf) {
        let a = self.root.join(&m.path);
        let b = self.backup(&m.path);
        if m.enable {
            (b, a)
        } else {
            (a, b)
        }
    }
    fn rollback(&self, j: &Journal) -> Result<()> {
        self.validate(&j.before)?;
        for m in j.moves.iter().rev() {
            validate_path(&m.path)?;
            let (src, dst) = self.move_paths(m);
            no_symlink_ancestors(src.parent().unwrap())?;
            no_symlink_ancestors(dst.parent().unwrap())?;
            match (exists(&src), exists(&dst)) {
                (true, false) if tree_digest(&src)? == m.digest => {}
                (false, true) if tree_digest(&dst)? == m.digest => {
                    fs::create_dir_all(src.parent().unwrap()).map_err(err)?;
                    rename_exclusive(&dst, &src)?;
                }
                _ => return Err(format!("{} 的位置或内容异常，未覆盖任何冲突文件", m.path)),
            }
        }
        self.persist(&j.before)?;
        fs::remove_file(self.vault.join("transaction.json")).map_err(err)?;
        self.certify(&j.before)
    }
    pub fn recover(&self) -> Result<()> {
        let _lock = self.lock()?;
        let p = self.vault.join("transaction.json");
        if exists(&p) {
            self.rollback(&read_json(&p)?)
        } else {
            let m = self.manifest()?;
            self.certify(&m)
        }
    }
}

pub const PROVIDERS: &[(&str, &str, &str)] = &[
    (".cursor/skills", "skill", "Cursor"),
    (".cursor/rules", "rule", "Cursor"),
    (".agents/skills", "skill", "Codex"),
    (".claude/skills", "skill", "Claude"),
    (".dsh/skills", "skill", "DeepSeek Harness"),
    (".zcode/skills", "skill", "ZCode"),
    (".kimi/skills", "skill", "Kimi"),
];
fn validate_path(path: &str) -> Result<(&'static str, &'static str)> {
    let p = Path::new(path);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) || path.contains('\\') {
        return Err("非法资源路径".into());
    }
    for (base, kind, provider) in PROVIDERS {
        if p.parent() == Some(Path::new(base))
            && (*kind != "rule" || p.extension().is_some_and(|s| s == "mdc"))
        {
            return Ok((kind, provider));
        }
    }
    Err(format!("不支持管理此路径：{path}"))
}
fn no_symlink_ancestors(p: &Path) -> Result<()> {
    for ancestor in p.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(ancestor) {
            if meta.file_type().is_symlink() {
                return Err(format!("暂不移动含软链接的路径：{}", ancestor.display()));
            }
        }
    }
    Ok(())
}
// File mtimes survive parking/restoring. Directory mtimes would also count
// filesystem housekeeping, so only include regular files and never follow links.
fn resource_updated_at(p: &Path) -> Option<u64> {
    let meta = fs::symlink_metadata(p).ok()?;
    if meta.is_dir() {
        fs::read_dir(p)
            .ok()?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if entry.file_name() == ".git" {
                    return None;
                }
                resource_updated_at(&entry.path())
            })
            .max()
    } else if meta.is_file() {
        let millis = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis();
        u64::try_from(millis).ok()
    } else {
        None
    }
}

pub fn tree_digest(p: &Path) -> Result<String> {
    fn visit(p: &Path, h: &mut Sha256) -> Result<()> {
        let meta = fs::symlink_metadata(p).map_err(err)?;
        if meta.file_type().is_symlink() {
            return Err(format!("暂不移动含软链接的资源：{}", p.display()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            h.update(meta.permissions().mode().to_le_bytes());
        }
        if meta.is_dir() {
            h.update(b"directory");
            let mut entries = fs::read_dir(p)
                .map_err(err)?
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(err)?;
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name();
                if name == ".git" {
                    return Err("资源含嵌套仓库，必须单独添加为项目".into());
                }
                let bytes = name.as_encoded_bytes();
                h.update((bytes.len() as u64).to_le_bytes());
                h.update(bytes);
                visit(&e.path(), h)?;
            }
        } else if meta.is_file() {
            h.update(b"file");
            h.update(meta.len().to_le_bytes());
            let mut f = File::open(p).map_err(err)?;
            let mut b = [0u8; 65536];
            loop {
                let n = f.read(&mut b).map_err(err)?;
                if n == 0 {
                    break;
                }
                h.update(&b[..n]);
            }
        } else {
            return Err("不能管理特殊文件".into());
        }
        Ok(())
    }
    let mut h = Sha256::new();
    visit(p, &mut h)?;
    Ok(format!("{:x}", h.finalize()))
}
fn read_limited(p: &Path) -> Result<String> {
    no_symlink_ancestors(p)?;
    let f = File::open(p).map_err(err)?;
    let mut bytes = Vec::new();
    f.take(256 * 1024).read_to_end(&mut bytes).map_err(err)?;
    Ok(String::from_utf8_lossy(&bytes).into())
}
pub fn frontmatter(content: &str, key: &str) -> Option<String> {
    let mut lines = content.trim_start_matches('\u{feff}').lines().peekable();
    if lines.next()?.trim() != "---" {
        return None;
    }
    while let Some(line) = lines.next() {
        if line.trim() == "---" {
            break;
        }
        if let Some(v) = line.strip_prefix(&format!("{key}:")) {
            let v = v.trim();
            if v.starts_with('>') || v.starts_with('|') {
                let mut parts = Vec::new();
                while let Some(next) = lines.peek() {
                    if !next.is_empty() && !next.starts_with([' ', '\t']) {
                        break;
                    }
                    parts.push(lines.next().unwrap().trim().to_string());
                }
                return Some(
                    parts
                        .join(if v.starts_with('|') { "\n" } else { " " })
                        .trim()
                        .to_string(),
                );
            }
            return Some(v.trim_matches(['\"', '\'']).to_string());
        }
    }
    None
}
#[cfg(unix)]
fn set_executable(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).map_err(err)
}
#[cfg(not(unix))]
fn set_executable(_p: &Path) -> Result<()> {
    Ok(())
}
#[cfg(unix)]
fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
fn executable(_p: &Path) -> bool {
    true
}
fn rename_exclusive(src: &Path, dst: &Path) -> Result<()> {
    // macOS provides an atomic no-replace rename, preventing a concurrent writer
    // from being overwritten between the preflight and the actual operation.
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(src.as_os_str().as_bytes()).map_err(err)?;
        let b = std::ffi::CString::new(dst.as_os_str().as_bytes()).map_err(err)?;
        if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) } != 0 {
            return Err(err(std::io::Error::last_os_error()));
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let a = std::ffi::CString::new(src.as_os_str().as_bytes()).map_err(err)?;
        let b = std::ffi::CString::new(dst.as_os_str().as_bytes()).map_err(err)?;
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
            return Err(err(std::io::Error::last_os_error()));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (src, dst);
        return Err("当前平台暂不支持安全移动".into());
    }
    sync_dir(src.parent().unwrap())?;
    sync_dir(dst.parent().unwrap())
}

pub const HOOK: &str = r#"#!/bin/sh
# Skill Manager commit guard v1 — keeps existing hook after this check.
set -eu
state=$(git rev-parse --path-format=absolute --git-path skill-manager)
fail() {
  echo 'Skill Manager：提交已拦截。请在管理器中“提交前全部恢复”，或检查恢复中心。' >&2
  echo "恢复资料：$state/RECOVERY.md（--no-verify 可显式绕过保护）" >&2
  exit 1
}
[ -s "$state/state.json" ] && [ -s "$state/ready.hash" ] || fail
[ ! -e "$state/transaction.json" ] || fail
actual=$(git hash-object "$state/state.json") || fail
expected=$(cat "$state/ready.hash") || fail
[ "$actual" = "$expected" ] || fail
hooks=$(git rev-parse --path-format=absolute --git-path hooks)
if [ -x "$hooks/pre-commit.skill-manager.previous" ]; then
  exec "$hooks/pre-commit.skill-manager.previous" "$@"
fi
exit 0
"#;
const RECOVERY: &str = r#"# Skill Manager 恢复说明

项目文件仍属于项目。关闭时原样移动到 resources/<路径的 SHA256>，不删除内容，不修改 Git index。
state.json 中 parked 键是原始相对路径，digest 校验内容及权限，head/index 记录关闭时 Git 状态。
transaction.json 表示中断操作；优先使用应用的“恢复中断操作”回滚整个批次。

如果应用不可用：先复制此目录做完整备份。检查 state.json（存在 transaction.json 时同时检查 before 和 moves），将 resources/ 对应内容移回原始路径。原位置已有文件时先比较并保留双方，绝不覆盖。用 git status / git diff / git diff --cached 检查工作树和暂存区，尤其是已暂存删除。不要只删除 ready.hash 或清单来绕过检查。

所有内容确认恢复后，可以移走 Skill Manager 的 pre-commit；若有 pre-commit.skill-manager.previous，把原 Hook 恢复到 pre-commit。无需修改 Git 索引标志或永久忽略资源。

core.hooksPath 自定义路径不自动接管。worktree 共享 hooks，但暂存目录独立；安装后其他 worktree 也须在应用中启用保护以建立各自清单，或由用户手动整合 hook。
git commit --no-verify 会主动绕过保护。工具不能阻止显式绕过或直接修改 Git 对象。
"#;
