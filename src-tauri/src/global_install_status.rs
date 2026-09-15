//! Read-only comparison of installation results with their recorded download sources.
use super::*;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStatus {
    pub key: String,
    pub ids: Vec<String>,
    pub status: String,
    pub message: String,
}
fn status(key: &str, ids: Vec<String>, state: &str, message: impl Into<String>) -> InstallStatus {
    InstallStatus {
        key: key.into(),
        ids,
        status: state.into(),
        message: message.into(),
    }
}
fn origins(record: &Record) -> Vec<Origin> {
    let mut sources: Vec<_> = record.origin.clone().into_iter().collect();
    if let DownloadSource::Remote { origin } = record.initial_source() {
        sources.push(origin);
    }
    sources
}
fn same_source(origin: &Origin, repo: &str, path: &str) -> bool {
    origin.repo.eq_ignore_ascii_case(repo) && origin.path == path
}
fn compare(
    records: &BTreeMap<String, Record>,
    repo: &str,
    path: &str,
    key: &str,
    tree: &Path,
) -> InstallStatus {
    let matches: Vec<_> = records
        .values()
        .filter(|r| origins(r).iter().any(|o| same_source(o, repo, path)))
        .collect();
    let ids = matches.iter().map(|r| r.id.clone()).collect();
    if matches.is_empty() {
        return status(key, ids, "unmatched", "未匹配到本地下载源");
    }
    if matches.len() > 1 {
        return status(
            key,
            ids,
            "multiple",
            "本地有多个相同来源的版本，请在我的技能中选择要比较的版本",
        );
    }
    let r = matches[0];
    let result: Result<(String, String)> = (|| {
        if path != "." {
            registry::relative(path)?;
        }
        let root = tree.canonicalize().map_err(e)?;
        let remote_path = root.join(path).canonicalize().map_err(e)?;
        if !remote_path.starts_with(&root) {
            return Err("Skill 路径超出下载的仓库".into());
        }
        Ok((digest(Path::new(&r.source))?, digest(&remote_path)?))
    })();
    match result {
        Err(error) => status(key, ids, "error", error),
        Ok((local, remote)) if local == remote => {
            status(key, ids, "latest", "本地与此下载源的完整内容一致，无需更新")
        }
        Ok((_, remote))
            if remote == r.baseline
                && r.update_origin()
                    .is_some_and(|o| same_source(&o, repo, path)) =>
        {
            status(key, ids, "local", "远端未变化，仅本地有修改，无需更新")
        }
        Ok((local, _)) => {
            let mut message = "此下载源与本地内容不同，可比较后更新".to_string();
            if local != r.baseline {
                message.push_str("；本地有修改，更新前请核对差异");
            }
            if r.update_origin()
                .is_some_and(|o| !same_source(&o, repo, path))
            {
                message.push_str("；当前升级来源已更改，选择此源更新将重新关联来源");
            }
            status(key, ids, "available", message)
        }
    }
}
impl Manager {
    pub fn market_install_status(
        &self,
        skills: &[registry::MarketSkill],
    ) -> Result<Vec<InstallStatus>> {
        self.market_install_status_with(skills, registry::fetch_repo)
    }
    fn market_install_status_with(
        &self,
        skills: &[registry::MarketSkill],
        fetch: impl Fn(&str, &str, &Path) -> Result<()>,
    ) -> Result<Vec<InstallStatus>> {
        if skills.len() > 30 {
            return Err("每次最多检查 30 条搜索结果".into());
        }
        let _lock = self.lock()?;
        let records = self.records()?;
        let mut repos: BTreeMap<String, Result<(tempfile::TempDir, Vec<Candidate>)>> =
            BTreeMap::new();
        let mut results = Vec::new();
        for skill in skills {
            registry::validate_repo(&skill.repo, "HEAD")?;
            let key = format!("{}/{}", skill.repo, skill.skill_id);
            if !records.values().any(|r| {
                origins(r)
                    .iter()
                    .any(|o| o.repo.eq_ignore_ascii_case(&skill.repo))
            }) {
                results.push(status(&key, vec![], "unmatched", "未匹配到本地下载源"));
                continue;
            }
            let fetched = repos.entry(skill.repo.to_lowercase()).or_insert_with(|| {
                let temp = tempfile::tempdir().map_err(e)?;
                fetch(&skill.repo, "HEAD", temp.path())?;
                let candidates = registry::candidates_with_fallback(
                    temp.path(),
                    skill.repo.rsplit('/').next().unwrap_or("skill"),
                )?;
                Ok((temp, candidates))
            });
            match fetched {
                Err(error) => results.push(status(
                    &key,
                    vec![],
                    "error",
                    format!("无法确认安装和更新状态：{error}"),
                )),
                Ok((temp, candidates)) => {
                    let matches: Vec<_> = candidates
                        .iter()
                        .filter(|c| c.path == skill.skill_id || c.name == skill.skill_id)
                        .collect();
                    if matches.len() != 1 {
                        results.push(status(
                            &key,
                            vec![],
                            "unknown",
                            "无法唯一确定搜索结果的实际路径，请查看详情选择具体 Skill",
                        ));
                    } else {
                        results.push(compare(
                            &records,
                            &skill.repo,
                            &matches[0].path,
                            &key,
                            temp.path(),
                        ));
                    }
                }
            }
        }
        Ok(results)
    }
    pub fn discovery_install_status(&self, discovery: &str) -> Result<Vec<InstallStatus>> {
        let _lock = self.lock()?;
        valid_token(discovery)?;
        let cache = self.data.join("discoveries").canonicalize().map_err(e)?;
        let dir = cache.join(discovery).canonicalize().map_err(e)?;
        if !dir.starts_with(&cache) {
            return Err("Skill 缓存路径已变化，请重新读取仓库".into());
        }
        let d: Discovery = read(&dir.join("discovery.json"))?;
        let tree = dir.join("tree").canonicalize().map_err(e)?;
        if !tree.starts_with(&dir) {
            return Err("Skill 缓存路径已变化，请重新读取仓库".into());
        }
        let records = self.records()?;
        Ok(d.candidates
            .iter()
            .map(|c| compare(&records, &d.repo, &c.path, &c.path, &tree))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn skill(root: &Path, text: &str) {
        fs::create_dir_all(root.join("references")).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\nBody",
        )
        .unwrap();
        fs::write(root.join("references/guide.md"), text).unwrap();
    }
    fn fixture() -> (tempfile::TempDir, Manager, Record) {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        fs::create_dir(&home).unwrap();
        let m = Manager::new(&home, &t.path().join("data")).unwrap();
        let input = t.path().join("demo");
        skill(&input, "old");
        let p = m.prepare_local(&input).unwrap();
        m.apply(&p.token, false).unwrap();
        let mut r = m.record(&p.record.id).unwrap();
        r.download_source = Some(DownloadSource::Remote {
            origin: Origin {
                repo: "example/repo".into(),
                reference: "HEAD".into(),
                path: "nested/demo".into(),
            },
        });
        m.save(&r).unwrap();
        (t, m, r)
    }
    fn query(repo: &str, id: &str) -> registry::MarketSkill {
        registry::MarketSkill {
            repo: repo.into(),
            name: "display name is not identity".into(),
            skill_id: id.into(),
            installs: 1,
        }
    }
    #[test]
    fn search_matches_download_source_and_compares_supporting_files_read_only() {
        let (t, m, r) = fixture();
        let before = serde_json::to_string(&m.record(&r.id).unwrap()).unwrap();
        let calls = Cell::new(0);
        let rows = m
            .market_install_status_with(
                &[
                    query("EXAMPLE/repo", "demo"),
                    query("example/repo", "nested/demo"),
                    query("other/repo", "demo"),
                ],
                |_, reference, dest| {
                    assert_eq!(reference, "HEAD");
                    calls.set(calls.get() + 1);
                    skill(&dest.join("nested/demo"), "new");
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(rows[0].status, "available");
        assert_eq!(rows[0].ids, vec![r.id.clone()]);
        assert_eq!(rows[1].status, "available");
        assert_eq!(rows[2].status, "unmatched");
        assert_eq!(
            before,
            serde_json::to_string(&m.record(&r.id).unwrap()).unwrap()
        );
        assert_eq!(
            fs::read_to_string(Path::new(&r.source).join("references/guide.md")).unwrap(),
            "old"
        );
        assert!(!t.path().join("data/discoveries").exists());
    }
    #[test]
    fn latest_local_edits_and_failures_are_distinct() {
        let (_t, m, r) = fixture();
        let queries = [query("example/repo", "demo")];
        let fetch = |_: &str, _: &str, dest: &Path| {
            skill(&dest.join("nested/demo"), "old");
            Ok(())
        };
        assert_eq!(
            m.market_install_status_with(&queries, fetch).unwrap()[0].status,
            "latest"
        );
        fs::write(
            Path::new(&r.source).join("references/guide.md"),
            "local edit",
        )
        .unwrap();
        assert_eq!(
            m.market_install_status_with(&queries, fetch).unwrap()[0].status,
            "local"
        );
        let error = m
            .market_install_status_with(&queries, |_, _, _| Err("offline".into()))
            .unwrap();
        assert_eq!(error[0].status, "error");
        assert!(error[0].message.contains("offline"));
        fs::remove_dir_all(&r.source).unwrap();
        assert_eq!(
            m.market_install_status_with(&queries, fetch).unwrap()[0].status,
            "error"
        );
    }
    #[test]
    fn ambiguous_names_and_different_paths_never_select_a_local_version() {
        let (_t, m, _) = fixture();
        let rows = m
            .market_install_status_with(
                &[
                    query("example/repo", "demo"),
                    query("example/repo", "other/demo"),
                ],
                |_, _, dest| {
                    skill(&dest.join("nested/demo"), "old");
                    skill(&dest.join("other/demo"), "old");
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(rows[0].status, "unknown");
        assert!(rows[0].ids.is_empty());
        assert_eq!(rows[1].status, "unmatched");
        assert!(rows[1].ids.is_empty());
    }
    #[test]
    fn discovery_status_and_update_preview_use_the_same_source_and_preserve_initial_origin() {
        let (t, m, mut r) = fixture();
        r.origin = Some(Origin {
            repo: "different/repo".into(),
            reference: "main".into(),
            path: "demo".into(),
        });
        m.save(&r).unwrap();
        let remote = t.path().join("remote");
        skill(&remote.join("nested/demo"), "new");
        let d = m.cache_discovery("example/repo", "HEAD", &remote).unwrap();
        let rows = m.discovery_install_status(&d.token).unwrap();
        assert_eq!(rows[0].status, "available");
        assert!(rows[0].message.contains("升级来源已更改"));
        let plan = m
            .prepare_remote(&d.token, &rows[0].key, Some(&rows[0].ids[0]))
            .unwrap();
        assert!(plan.old_hash.is_some());
        m.apply(&plan.token, true).unwrap();
        assert_eq!(m.record(&r.id).unwrap().download_source, r.download_source);
        assert_eq!(
            m.discovery_install_status(&d.token).unwrap()[0].status,
            "latest"
        );
    }
    #[test]
    fn multiple_local_versions_require_explicit_selection() {
        let (t, m, r) = fixture();
        let mut other = r.clone();
        other.id = "other".into();
        other.source = t.path().join("other").to_string_lossy().into();
        skill(Path::new(&other.source), "other");
        m.save(&other).unwrap();
        let rows = m
            .market_install_status_with(&[query("example/repo", "demo")], |_, _, dest| {
                skill(&dest.join("nested/demo"), "new");
                Ok(())
            })
            .unwrap();
        assert_eq!(rows[0].status, "multiple");
        assert_eq!(rows[0].ids.len(), 2);
    }
}
