//! Duplicate sources are consolidated only after an explicit, reversible preview.
use super::*;
#[path = "global_diff.rs"]
mod diff;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateSource {
    pub id: String,
    pub source: String,
    pub digest: Option<String>,
    pub problem: Option<String>,
    pub managed: bool,
    pub origin: Option<Origin>,
    pub tree: Option<BTreeMap<String, String>>,
    pub permissions: Option<BTreeMap<String, u32>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub name: String,
    pub sources: Vec<DuplicateSource>,
}
#[derive(Serialize)]
pub struct DuplicateReport {
    pub groups: Vec<DuplicateGroup>,
    pub warnings: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateChoice {
    pub keep_id: String,
    pub duplicate_ids: Vec<String>,
    #[serde(default)]
    pub allow_different: bool,
}

// Compare the entire tree, including executable bits and all Unix permissions.
// Require real roots so a stale preview cannot replace an alias or another source.
fn duplicate_snapshot(path: &Path) -> Result<(BTreeMap<String, String>, BTreeMap<String, u32>)> {
    if !fs::symlink_metadata(path).map_err(e)?.is_dir() {
        return Err("源目录已变为软链接或不再是目录，请重新检查".into());
    }
    let tree = safe_tree(path)?;
    #[cfg(unix)]
    let modes: BTreeMap<_, _> = {
        use std::os::unix::fs::PermissionsExt;
        tree.keys()
            .map(|rel| {
                let p = path.join(rel.trim_end_matches('/'));
                Ok((
                    rel.clone(),
                    fs::symlink_metadata(p).map_err(e)?.permissions().mode() & 0o7777,
                ))
            })
            .collect::<Result<_>>()?
    };
    #[cfg(not(unix))]
    let modes: BTreeMap<String, u32> = BTreeMap::new();
    Ok((tree, modes))
}
pub(super) fn duplicate_digest(path: &Path) -> Result<String> {
    Ok(hash(
        &serde_json::to_vec(&duplicate_snapshot(path)?).map_err(e)?,
    ))
}
impl Manager {
    pub fn duplicate_report(&self) -> Result<DuplicateReport> {
        let _lock = self.lock()?;
        let inventory = self.inventory()?;
        let mut grouped: BTreeMap<String, BTreeMap<PathBuf, Skill>> = BTreeMap::new();
        for skill in inventory.skills {
            let path = Path::new(&skill.source)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&skill.source));
            grouped
                .entry(skill.name.clone())
                .or_default()
                .entry(path)
                .or_insert(skill);
        }
        let library = self
            .library()
            .canonicalize()
            .unwrap_or_else(|_| self.library());
        let mut groups = vec![];
        for (name, sources) in grouped {
            if sources.len() < 2 {
                continue;
            }
            let mut sources: Vec<_> = sources
                .into_iter()
                .map(|(path, s)| {
                    let snapshot = duplicate_snapshot(&path);
                    let digest = snapshot
                        .as_ref()
                        .map(|value| {
                            hash(&serde_json::to_vec(value).expect("snapshot serialization"))
                        })
                        .map_err(Clone::clone);
                    DuplicateSource {
                        id: s.id,
                        source: path.to_string_lossy().into(),
                        problem: digest.as_ref().err().cloned(),
                        digest: digest.ok(),
                        managed: s.managed,
                        origin: s.origin,
                        tree: snapshot.as_ref().ok().map(|(tree, _)| tree.clone()),
                        permissions: snapshot.ok().map(|(_, permissions)| permissions),
                    }
                })
                .collect();
            sources.sort_by_key(|s| {
                (
                    Path::new(&s.source).parent() != Some(library.as_path()),
                    s.source.clone(),
                )
            });
            groups.push(DuplicateGroup { name, sources });
        }
        Ok(DuplicateReport {
            groups,
            warnings: inventory.warnings,
        })
    }

    pub fn prepare_duplicates(&self, choices: Vec<DuplicateChoice>) -> Result<AdminPlan> {
        let _lock = self.lock()?;
        let mut p = self.admin_plan("deduplicate")?;
        let inventory = self.inventory()?;
        let skills: BTreeMap<_, _> = inventory.skills.iter().map(|s| (s.id.clone(), s)).collect();
        let mut used = BTreeSet::new();
        for choice in choices {
            let keep = skills
                .get(&choice.keep_id)
                .ok_or("保留源已不在扫描结果中，请重新检查")?;
            if choice.duplicate_ids.is_empty() {
                return Err("请选择要合并的副本".into());
            }
            let source = Path::new(&keep.source);
            let signature = duplicate_digest(source)?;
            if source.canonicalize().map_err(e)? != source {
                return Err("源路径已变化，请重新检查".into());
            }
            if !used.insert(source.to_path_buf()) {
                return Err("同一来源不能参与多组合并".into());
            }
            p.duplicate_checks.insert(source.into(), signature.clone());
            p.checks.insert(source.into(), Some(fingerprint(source)?));
            let mut record = p.after.records.get(&keep.id).cloned();
            for id in choice.duplicate_ids {
                let other = skills.get(&id).ok_or("副本已不在扫描结果中，请重新检查")?;
                let path = Path::new(&other.source);
                if !used.insert(path.to_path_buf())
                    || source.starts_with(path)
                    || path.starts_with(source)
                {
                    return Err("重复或嵌套路径不能合并".into());
                }
                let other_signature = duplicate_digest(path)?;
                if other.name != keep.name
                    || (!choice.allow_different && other_signature != signature)
                {
                    return Err(format!(
                        "{} 的名称、完整内容或权限不同，保留双方，不会合并",
                        other.source
                    ));
                }
                if path.canonicalize().map_err(e)? != path {
                    return Err("副本路径已变化，请重新检查".into());
                }
                p.duplicate_checks
                    .insert(path.into(), other_signature.clone());
                if choice.allow_different {
                    p.warnings.push(format!("{}：统一使用 {}。{} 中的内容、权限和更新来源不再独立生效；原版本会完整备份。", keep.name, source.display(), path.display()));
                }
                if let Some(old) = p.after.records.remove(&id) {
                    if let Some(current) = &mut record {
                        if !choice.allow_different
                            && (current.origin != old.origin || current.baseline != old.baseline)
                        {
                            return Err(
                                "内容相同，但更新来源或版本记录不同。请先统一来源信息，再合并"
                                    .into(),
                            );
                        }
                        current.expected.extend(old.expected);
                    } else {
                        record = Some(Record {
                            id: keep.id.clone(),
                            name: keep.name.clone(),
                            source: keep.source.clone(),
                            download_source: Some(if choice.allow_different {
                                DownloadSource::Unknown
                            } else {
                                old.initial_source()
                            }),
                            origin: if choice.allow_different {
                                keep.origin.clone()
                            } else {
                                old.origin
                            },
                            baseline: if choice.allow_different {
                                digest(source)?
                            } else {
                                old.baseline
                            },
                            expected: old.expected,
                        });
                    }
                }
                let old_group = p.after.config.members.remove(&id);
                if let Some(group) = old_group.filter(|_| !choice.allow_different) {
                    if p.after
                        .config
                        .members
                        .get(&keep.id)
                        .is_some_and(|g| g != &group)
                    {
                        return Err("副本属于不同分组，请先统一分组，再合并".into());
                    }
                    p.after.config.members.insert(keep.id.clone(), group);
                }
                // Keep original metadata under its old ID for later migration and undo.
                for value in p.after.config.aliases.values_mut() {
                    if value == &id {
                        *value = keep.id.clone();
                    }
                }
                p.after.config.aliases.insert(id.clone(), keep.id.clone());
                let projections: Vec<_> = p
                    .after
                    .config
                    .copies
                    .iter()
                    .filter(|(_, copy)| copy.source == other.source)
                    .map(|(dest, copy)| (dest.clone(), copy.clone()))
                    .collect();
                for (dest, copy) in projections {
                    if choice.allow_different {
                        let dest = Path::new(&dest);
                        if !fs::symlink_metadata(dest).map_err(e)?.is_dir()
                            || digest(dest)? != copy.digest
                        {
                            return Err(format!(
                                "客户端副本有新修改，请先处理后再统一版本：{}",
                                dest.display()
                            ));
                        }
                        let backup = self
                            .admin_dir(&p.token)
                            .join(format!("duplicate-client-backup-{}", p.moves.len()));
                        self.checked_move(&mut p, dest, &backup)?;
                        let stage = self
                            .admin_dir(&p.token)
                            .join(format!("duplicate-client-link-{}", p.moves.len()));
                        create_link(source, &stage)?;
                        self.checked_move(&mut p, &stage, dest)?;
                        p.after
                            .config
                            .copies
                            .remove(&dest.to_string_lossy().to_string());
                        p.summary.push(format!(
                            "客户端入口 {}：备份至 {}，改用 {}",
                            dest.display(),
                            backup.display(),
                            source.display()
                        ));
                    } else if let Some(copy) = p.after.config.copies.get_mut(&dest) {
                        copy.source = keep.source.clone();
                    }
                }
                let backup = self
                    .admin_dir(&p.token)
                    .join(format!("duplicate-backup-{}", p.moves.len()));
                self.checked_move(&mut p, path, &backup)?;
                let stage = self
                    .admin_dir(&p.token)
                    .join(format!("duplicate-link-{}", p.moves.len()));
                create_link(source, &stage)?;
                self.checked_move(&mut p, &stage, path)?;
                p.summary.push(format!(
                    "{}：保留 {}；将 {} 备份至 {}，原位置改为软链接",
                    keep.name,
                    source.display(),
                    path.display(),
                    backup.display()
                ));
            }
            if let Some(record) = record {
                p.after.records.insert(keep.id.clone(), record);
            }
        }
        if p.moves.is_empty() {
            return Err("请选择至少一组要处理的副本".into());
        }
        p.warnings.push("合并后，修改保留源会影响所有链接到它的客户端。原副本完整保存在备份中，可从备份中心预览整批回退。".into());
        self.finish_plan(p)
    }
}
