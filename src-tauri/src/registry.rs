//! Bounded public registry access. Search results identify repositories; exact
//! resource paths are resolved from the downloaded tree, never guessed by basename.
use crate::{
    engine::{frontmatter, Result},
    global::{safe_tree, validate_name},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    pub repo: String,
    pub reference: String,
    pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub name: String,
    pub description: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketSkill {
    pub name: String,
    pub skill_id: String,
    pub repo: String,
    pub installs: u64,
}
#[derive(Serialize, Deserialize)]
pub struct SearchResult {
    pub skills: Vec<MarketSkill>,
    pub count: usize,
    pub query: String,
}
pub fn validate_repo(repo: &str, reference: &str) -> Result<()> {
    let parts: Vec<_> = repo.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|s| {
            s.is_empty()
                || s.len() > 100
                || *s == "."
                || *s == ".."
                || s.starts_with('-')
                || !s
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        })
    {
        return Err("请输入 GitHub 仓库 owner/repo".into());
    }
    if reference.is_empty()
        || reference.len() > 200
        || reference.contains("..")
        || reference.starts_with('/')
        || reference.ends_with('/')
        || !reference
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
    {
        return Err("仓库分支或标签格式不正确".into());
    }
    Ok(())
}
pub fn relative(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains('\\')
        || path.contains('\0')
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("非法仓库资源路径".into());
    }
    Ok(())
}
fn download(url: reqwest::Url, limit: u64) -> Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("SkillManager/0.2")
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| network_error(&e))?;
    let mut data = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > limit {
        return Err("远端内容超过下载大小限制".into());
    }
    Ok(data)
}

fn network_error(error: &reqwest::Error) -> String {
    use std::error::Error;

    let mut details = error.to_string();
    let mut cause = error.source();
    while let Some(source) = cause {
        details.push_str(" → ");
        details.push_str(&source.to_string());
        cause = source.source();
    }
    let hint = if error.is_timeout() {
        "连接或读取超时，请稍后重试。"
    } else if error.is_connect() {
        "无法建立连接，请检查网络、代理或证书设置后重试。"
    } else if error.is_status() {
        "远端服务返回错误状态，请稍后重试。"
    } else {
        "请稍后重试；若持续失败，请保留此错误详情。"
    };
    format!("网络请求失败：{details}。{hint}")
}

#[cfg(test)]
mod network_tests {
    use super::*;
    use std::{error::Error, net::TcpListener};

    #[test]
    fn connection_error_keeps_underlying_cause() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        // Close an accepted connection before HTTP headers arrive.
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            drop(stream);
        });
        let error = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap()
            .get(format!("http://{address}/"))
            .send()
            .unwrap_err();
        server.join().unwrap();
        let mut cause = error.source().expect("transport cause");
        while let Some(next) = cause.source() {
            cause = next;
        }
        let message = network_error(&error);
        assert!(message.contains(&cause.to_string()), "{message}");
    }
}
pub fn search(query: &str, offset: usize) -> Result<SearchResult> {
    if query.trim().is_empty() || query.len() > 200 || offset > 10000 {
        return Err("请输入 1–200 字符的搜索词".into());
    }
    let url = reqwest::Url::parse_with_params(
        "https://skills.sh/api/search",
        &[
            ("q", query),
            ("limit", "30"),
            ("offset", &offset.to_string()),
        ],
    )
    .map_err(|e| e.to_string())?;
    parse_search(&download(url, 2 * 1024 * 1024)?, query)
}
pub fn parse_search(data: &[u8], query: &str) -> Result<SearchResult> {
    let value: serde_json::Value =
        serde_json::from_slice(data).map_err(|e| format!("搜索服务返回格式异常：{e}"))?;
    let array = value
        .get("skills")
        .and_then(|v| v.as_array())
        .ok_or("搜索服务没有返回 skills 列表")?;
    let skills = array
        .iter()
        .filter_map(|v| {
            let repo = v.get("source")?.as_str()?.to_string();
            validate_repo(&repo, "HEAD").ok()?;
            Some(MarketSkill {
                name: v.get("name")?.as_str()?.into(),
                skill_id: v.get("skillId")?.as_str()?.into(),
                repo,
                installs: v.get("installs").and_then(|v| v.as_u64()).unwrap_or(0),
            })
        })
        .collect();
    Ok(SearchResult {
        skills,
        count: value
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(array.len() as u64) as usize,
        query: query.into(),
    })
}
pub fn fetch_repo(repo: &str, reference: &str, destination: &Path) -> Result<()> {
    validate_repo(repo, reference)?;
    let mut url = reqwest::Url::parse("https://codeload.github.com").unwrap();
    {
        let mut segments = url.path_segments_mut().map_err(|_| "URL 构造失败")?;
        for p in repo.split('/') {
            segments.push(p);
        }
        segments.push("zip").push(reference);
    }
    extract_archive(&download(url, 64 * 1024 * 1024)?, destination)
}
pub fn extract_archive(data: &[u8], dest: &Path) -> Result<()> {
    extract(data, dest, true)
}
pub fn extract_local_archive(data: &[u8], dest: &Path) -> Result<()> {
    extract(data, dest, false)
}
fn extract(data: &[u8], dest: &Path, strip_root: bool) -> Result<()> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(data)).map_err(|e| format!("无效 ZIP：{e}"))?;
    if archive.len() > 20000 {
        return Err("归档文件数超过限制".into());
    }
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut total = 0u64;
    let mut links = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut root = None;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        let path = entry.enclosed_name().ok_or("归档含越界路径")?;
        if name.contains('\\')
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err("归档含不安全路径".into());
        }
        let mut parts = path.components();
        let first = parts
            .next()
            .ok_or("空归档路径")?
            .as_os_str()
            .to_string_lossy()
            .to_string();
        if strip_root {
            if let Some(r) = &root {
                if r != &first {
                    return Err("GitHub 归档包含多个根目录".into());
                }
            } else {
                root = Some(first);
            }
        }
        let relative = if strip_root {
            parts.as_path()
        } else {
            path.as_path()
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        if relative.components().any(|c| c.as_os_str() == ".git") {
            return Err("归档含嵌套 Git 数据".into());
        }
        if !seen.insert(relative.to_string_lossy().to_lowercase()) {
            return Err("归档含重复或大小写冲突路径".into());
        }
        let target = dest.join(relative);
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        let mode = entry.unix_mode().unwrap_or(0o644);
        if mode & 0o170000 == 0o120000 {
            let mut value = String::new();
            entry
                .by_ref()
                .take(4097)
                .read_to_string(&mut value)
                .map_err(|e| e.to_string())?;
            if value.len() > 4096 {
                return Err("软链接目标过长".into());
            }
            total += value.len() as u64;
            links.push((target, value));
            continue;
        }
        if mode & 0o170000 != 0 && mode & 0o170000 != 0o100000 {
            return Err("归档包含特殊文件".into());
        }
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)
            .map_err(|e| e.to_string())?;
        let mut buffer = [0; 65536];
        loop {
            let n = entry.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > 256 * 1024 * 1024 {
                return Err("解压内容超过 256 MB 限制".into());
            }
            output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(if mode & 0o111 != 0 { 0o755 } else { 0o644 }),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    for (path, target) in links {
        crate::global::create_link(Path::new(&target), &path)?;
    }
    safe_tree(dest)?;
    Ok(())
}
pub fn candidates(root: &Path) -> Result<Vec<Candidate>> {
    candidates_with_fallback(root, "skill")
}
pub fn candidates_with_fallback(root: &Path, fallback: &str) -> Result<Vec<Candidate>> {
    fn visit(
        root: &Path,
        p: &Path,
        out: &mut Vec<Candidate>,
        depth: usize,
        fallback: &str,
    ) -> Result<()> {
        if depth > 20 {
            return Ok(());
        }
        if p.join("SKILL.md").is_file() {
            // A Skill at the repository root uses a stable explicit "." selector.
            let rel = p
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .to_string();
            let mut text = String::new();
            fs::File::open(p.join("SKILL.md"))
                .map_err(|e| e.to_string())?
                .take(256 * 1024)
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            let name = if p == root {
                frontmatter(&text, "name")
                    .filter(|n| validate_name(n).is_ok())
                    .unwrap_or_else(|| fallback.into())
            } else {
                p.file_name().unwrap_or_default().to_string_lossy().into()
            };
            if validate_name(&name).is_ok() {
                out.push(Candidate {
                    path: if rel.is_empty() { ".".into() } else { rel },
                    name,
                    description: frontmatter(&text, "description").unwrap_or_default(),
                });
            }
            return Ok(());
        }
        for e in fs::read_dir(p).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            if e.file_type().map_err(|e| e.to_string())?.is_dir()
                && ![".git", "node_modules"].contains(&e.file_name().to_string_lossy().as_ref())
            {
                visit(root, &e.path(), out, depth + 1, fallback)?;
            }
        }
        Ok(())
    }
    let mut result = Vec::new();
    visit(root, root, &mut result, 0, fallback)?;
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

/// Build a public GitHub source page from validated repository metadata.
pub fn source_page_url(origin: &Origin) -> Result<String> {
    validate_repo(&origin.repo, &origin.reference)?;
    if origin.path != "." {
        relative(&origin.path)?;
    }
    let mut url = reqwest::Url::parse("https://github.com").map_err(|e| e.to_string())?;
    {
        let mut segments = url.path_segments_mut().map_err(|_| "来源网址无效")?;
        for segment in origin.repo.split('/') {
            segments.push(segment);
        }
        segments.push("tree").push(&origin.reference);
        if origin.path != "." {
            for segment in origin.path.split('/') {
                segments.push(segment);
            }
        }
    }
    Ok(url.to_string())
}

/// Branches and commit pins stay on their recorded ref. Version tags discover
/// newer stable GitHub releases; preview and checking share this resolution.
pub fn resolve_update_origin(origin: &Origin) -> Result<Origin> {
    validate_repo(&origin.repo, &origin.reference)?;
    if version_tag(&origin.reference).is_none() {
        return Ok(origin.clone());
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent("SkillManager/0.4")
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(format!(
            "https://api.github.com/repos/{}/releases/latest",
            origin.repo
        ))
        .send()
        .map_err(|e| network_error(&e))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(origin.clone());
    }
    let mut data = Vec::new();
    response
        .error_for_status()
        .map_err(|e| network_error(&e))?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > 1024 * 1024 {
        return Err("发布信息超过大小限制".into());
    }
    let release: Release = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    Ok(select_release(origin, &release))
}
fn version_tag(reference: &str) -> Option<semver::Version> {
    semver::Version::parse(reference.strip_prefix('v').unwrap_or(reference)).ok()
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}
fn select_release(origin: &Origin, release: &Release) -> Origin {
    let mut result = origin.clone();
    if !release.draft && !release.prerelease {
        if let (Some(current), Some(latest)) = (
            version_tag(&origin.reference),
            version_tag(&release.tag_name),
        ) {
            if latest > current
                && latest.pre.is_empty()
                && validate_repo(&origin.repo, &release.tag_name).is_ok()
            {
                result.reference = release.tag_name.clone();
            }
        }
    }
    result
}

#[cfg(test)]
mod release_tests {
    use super::*;
    #[test]
    fn discovers_newer_stable_release_without_downgrading_or_moving_branches() {
        let mut origin = Origin {
            repo: "example/repo".into(),
            reference: "v1.0.60".into(),
            path: "skills/demo".into(),
        };
        let mut release = Release {
            tag_name: "v1.0.61".into(),
            draft: false,
            prerelease: false,
        };
        assert_eq!(select_release(&origin, &release).reference, "v1.0.61");
        release.prerelease = true;
        assert_eq!(select_release(&origin, &release), origin);
        release.prerelease = false;
        for reference in ["main", "HEAD", "v1.0.62", "v1.0.61", "abcdef123456"] {
            origin.reference = reference.into();
            assert_eq!(select_release(&origin, &release), origin);
        }
    }
}
