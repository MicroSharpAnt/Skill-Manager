//! Read-only comparisons of two inventory sources. Diff runs on bounded temporary
//! text copies, never on repository paths or through a shell.
use super::*;
use std::process::Command;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareFile {
    path: String,
    status: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareSide {
    kind: String,
    permissions: u32,
    bytes: u64,
    line_endings: String,
    notice: Option<String>,
    #[serde(skip)]
    text: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillComparison {
    left_source: String,
    right_source: String,
    files: Vec<CompareFile>,
    path: Option<String>,
    left: Option<CompareSide>,
    right: Option<CompareSide>,
    patch: String,
    notice: Option<String>,
}
const MAX_TEXT: u64 = 1024 * 1024;
fn normalized(tree: BTreeMap<String, String>) -> BTreeMap<String, String> {
    tree.into_iter()
        .map(|(k, v)| (k.trim_end_matches('/').to_string(), v))
        .collect()
}
fn side(root: &Path, path: &str, tree: &BTreeMap<String, String>) -> Result<Option<CompareSide>> {
    let Some(expected) = tree.get(path) else {
        return Ok(None);
    };
    let file = root.join(path);
    let meta = fs::symlink_metadata(&file).map_err(e)?;
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o7777
    };
    #[cfg(not(unix))]
    let permissions = 0;
    let mut result = CompareSide {
        kind: "directory".into(),
        permissions,
        bytes: meta.len(),
        line_endings: String::new(),
        notice: None,
        text: None,
    };
    if meta.file_type().is_symlink() {
        let target = fs::read_link(&file)
            .map_err(e)?
            .to_string_lossy()
            .to_string();
        if *expected != format!("link:{target}") {
            return Err("比较期间链接已变化，请重新读取".into());
        }
        result.kind = "symlink".into();
        result.text = Some(target);
    } else if meta.is_file() {
        if !file.canonicalize().map_err(e)?.starts_with(root) {
            return Err("文件已指向 Skill 目录外".into());
        }
        result.kind = "file".into();
        if meta.len() > MAX_TEXT {
            result.notice = Some("文件超过 1 MB，未生成逐行 Diff；请在外部编辑器比较".into());
            return Ok(Some(result));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut bytes = vec![];
        options
            .open(&file)
            .map_err(e)?
            .take(MAX_TEXT + 1)
            .read_to_end(&mut bytes)
            .map_err(e)?;
        if bytes.len() as u64 > MAX_TEXT || !expected.ends_with(&format!(":{}", hash(&bytes))) {
            return Err("比较期间文件已变化，请重新读取".into());
        }
        if bytes.contains(&0) {
            result.notice = Some("二进制文件，不生成文本 Diff".into());
        } else if let Ok(text) = String::from_utf8(bytes) {
            let crlf = text.matches("\r\n").count();
            let lf = text.matches('\n').count();
            result.line_endings = if text.replace("\r\n", "").contains('\r') {
                "含 CR"
            } else if crlf > 0 && crlf == lf {
                "CRLF"
            } else if crlf > 0 {
                "混合 LF / CRLF"
            } else {
                "LF"
            }
            .into();
            if text.lines().count() > 12000 {
                result.notice =
                    Some("文件超过 12,000 行，未生成逐行 Diff；请在外部编辑器比较".into());
            } else {
                result.text = Some(text);
            }
        } else {
            result.notice = Some("文件不是 UTF-8 文本，不生成文本 Diff".into());
        }
    } else if !meta.is_dir() || expected != "directory" {
        return Err("文件类型已变化，请重新读取".into());
    }
    Ok(Some(result))
}
impl Manager {
    pub fn compare_skills(
        &self,
        left_id: &str,
        right_id: &str,
        path: Option<String>,
    ) -> Result<SkillComparison> {
        let _lock = self.lock()?;
        let inventory = self.inventory()?;
        let left = inventory
            .skills
            .iter()
            .find(|s| s.id == left_id)
            .ok_or("左侧来源已不存在，请重新检查")?;
        let right = inventory
            .skills
            .iter()
            .find(|s| s.id == right_id)
            .ok_or("右侧来源已不存在，请重新检查")?;
        if left_id == right_id || left.name != right.name {
            return Err("请选择同名 Skill 的两个不同来源".into());
        }
        let a = Path::new(&left.source).canonicalize().map_err(e)?;
        let b = Path::new(&right.source).canonicalize().map_err(e)?;
        if a == b {
            return Err("这两个入口已指向同一个实际目录".into());
        }
        compare_roots(&a, &b, path)
    }
    pub fn compare_plan(&self, token: &str, path: Option<String>) -> Result<SkillComparison> {
        let _lock = self.lock()?;
        self.ready()?;
        valid_token(token)?;
        let plans = self.data.join("plans").canonicalize().map_err(e)?;
        let dir = plans.join(token).canonicalize().map_err(e)?;
        if !dir.starts_with(&plans) {
            return Err("预览目录已变化，请重新预览".into());
        }
        let plan: crate::global::Plan = read(&dir.join("plan.json"))?;
        if plan.token != token || plan.record.id != hash(plan.record.source.as_bytes()) {
            return Err("预览身份不一致，请重新预览".into());
        }
        self.ensure_shared(Path::new(&plan.record.source))?;
        if self.records()?.get(&plan.record.id) != plan.before.as_ref() {
            return Err("预览后 Skill 管理状态已变化，请重新预览".into());
        }
        let source = PathBuf::from(&plan.record.source);
        let payload = dir.join("payload").canonicalize().map_err(e)?;
        if !payload.starts_with(&dir) {
            return Err("待安装目录已变化，请重新预览".into());
        }
        let validate = || -> Result<()> {
            if digest(&payload)? != plan.new_hash {
                return Err("待安装内容已变化，请重新预览".into());
            }
            if let Some(expected) = &plan.old_hash {
                if fs::symlink_metadata(&source)
                    .map_err(e)?
                    .file_type()
                    .is_symlink()
                    || digest(&source)? != *expected
                {
                    return Err("预览后本地内容发生变化，请重新预览".into());
                }
            } else if exists(&source) {
                return Err("安装目标已出现同名内容，请重新预览".into());
            }
            Ok(())
        };
        validate()?;
        let empty = tempfile::tempdir().map_err(e)?;
        let left = if plan.old_hash.is_some() {
            source.canonicalize().map_err(e)?
        } else {
            empty.path().canonicalize().map_err(e)?
        };
        let result = compare_roots(&left, &payload, path)?;
        validate()?;
        Ok(result)
    }
}
fn compare_roots(a: &Path, b: &Path, path: Option<String>) -> Result<SkillComparison> {
    let (at, am) = duplicate_snapshot(&a)?;
    let (bt, bm) = duplicate_snapshot(&b)?;
    let at = normalized(at);
    let bt = normalized(bt);
    let am: BTreeMap<_, _> = am
        .into_iter()
        .map(|(k, v)| (k.trim_end_matches('/').to_string(), v))
        .collect();
    let bm: BTreeMap<_, _> = bm
        .into_iter()
        .map(|(k, v)| (k.trim_end_matches('/').to_string(), v))
        .collect();
    let paths: BTreeSet<_> = at.keys().chain(bt.keys()).cloned().collect();
    let files = paths
        .iter()
        .map(|p| CompareFile {
            path: p.clone(),
            status: if !at.contains_key(p) {
                "added"
            } else if !bt.contains_key(p) {
                "removed"
            } else if at[p] != bt[p] {
                "changed"
            } else if am.get(p) != bm.get(p) {
                "permissions"
            } else {
                "same"
            }
            .into(),
        })
        .collect();
    let mut result = SkillComparison {
        left_source: a.to_string_lossy().into(),
        right_source: b.to_string_lossy().into(),
        files,
        path: path.clone(),
        left: None,
        right: None,
        patch: String::new(),
        notice: None,
    };
    let Some(path) = path else { return Ok(result) };
    if !paths.contains(&path)
        || Path::new(&path)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("无效比较路径，请选择清单中的文件".into());
    }
    result.left = side(&a, &path, &at)?;
    result.right = side(&b, &path, &bt)?;
    if result.left.as_ref().is_some_and(|s| s.notice.is_some())
        || result.right.as_ref().is_some_and(|s| s.notice.is_some())
    {
        result.notice = Some(
            "当前文件无法生成文本 Diff，原因见两侧文件信息。文件仍会完整参与版本比较与备份。"
                .into(),
        );
        return Ok(result);
    }
    let lt = result
        .left
        .as_ref()
        .and_then(|s| s.text.as_deref())
        .unwrap_or("");
    let rt = result
        .right
        .as_ref()
        .and_then(|s| s.text.as_deref())
        .unwrap_or("");
    if lt == rt {
        result.notice = Some("文本内容相同；如有权限或类型差异，请查看上方信息。".into());
        return Ok(result);
    }
    let temp = tempfile::tempdir().map_err(e)?;
    fs::write(temp.path().join("left"), lt).map_err(e)?;
    fs::write(temp.path().join("right"), rt).map_err(e)?;
    let output = Command::new("/usr/bin/diff")
        .args(["-u", "-L", "left", "-L", "right", "left", "right"])
        .current_dir(temp.path())
        .env_remove("DIFF_OPTIONS")
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("无法运行系统 Diff：{e}"))?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!(
            "Diff 失败：{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    result.patch = String::from_utf8(output.stdout).map_err(e)?;
    Ok(result)
}
