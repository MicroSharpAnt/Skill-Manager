use rusqlite::Connection;
use skill_manager::{
    global::admin::Repo,
    global::{Discovery, Manager},
    registry::{self, Candidate},
};
use std::{
    fs,
    io::{Cursor, Write},
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};
fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}
fn skill(p: &Path, s: &str) {
    write(
        &p.join("SKILL.md"),
        &format!("---\nname: demo\ndescription: Sample\n---\n{s}"),
    );
    write(&p.join("ref/a.md"), s);
}
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Manager) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let home = root.join("home");
    let data = root.join("data");
    fs::create_dir(&home).unwrap();
    let m = Manager::new(&home, &data).unwrap();
    (t, home, data, m)
}
#[test]
fn download_sources_preserve_files_and_groups_and_can_be_undone() {
    use skill_manager::{global::DownloadSource, registry::Origin};
    let (_t, home, _data, m) = fixture();
    let path = home.join(".codex/skills/demo");
    skill(&path, "keep this content");
    let original = fs::read(path.join("SKILL.md")).unwrap();
    let id = m.inventory().unwrap().skills[0].id.clone();
    let source = DownloadSource::Remote { origin: Origin {
        repo: "example/skills".into(), reference: "main".into(), path: "skills/demo".into(),
    }};
    let p = m.prepare_download_sources([(id.clone(), source.clone())].into()).unwrap();
    assert!(p.moves.is_empty());
    assert_eq!(p.before.config, p.after.config);
    assert_eq!(m.inventory().unwrap().skills[0].download_source, DownloadSource::Unknown);
    m.apply_admin(&p.token).unwrap();
    assert_eq!(m.inventory().unwrap().skills[0].download_source, source);
    assert_eq!(fs::read(path.join("SKILL.md")).unwrap(), original);
    assert!(m.prepare_download_sources([(id.clone(), source.clone())].into()).is_err());
    let undo = m.undo_admin(&p.token).unwrap();
    m.apply_admin(&undo.token).unwrap();
    assert_eq!(m.inventory().unwrap().skills[0].download_source, DownloadSource::Unknown);
    let stale = m.prepare_download_sources([(id, source)].into()).unwrap();
    skill(&path, "changed after preview");
    assert!(m.apply_admin(&stale.token).is_err());
}
fn install(t: &tempfile::TempDir, m: &Manager) -> String {
    let dir = t.path().join("demo");
    skill(&dir, "one");
    let p = m.prepare_local(&dir).unwrap();
    m.apply(&p.token, false).unwrap();
    p.record.id
}
fn cc(home: &Path) -> PathBuf {
    let dir = home.join(".cc-switch");
    fs::create_dir_all(&dir).unwrap();
    let db = Connection::open(dir.join("cc-switch.db")).unwrap();
    db.execute_batch("CREATE TABLE skills(id TEXT,name TEXT,directory TEXT,repo_owner TEXT,repo_name TEXT,repo_branch TEXT,readme_url TEXT,group_id TEXT,enabled_claude INTEGER,enabled_codex INTEGER,content_hash TEXT); CREATE TABLE skill_groups(id TEXT,name TEXT,color TEXT); CREATE TABLE skill_repos(owner TEXT,name TEXT,branch TEXT,enabled INTEGER); INSERT INTO skill_groups VALUES('group','Writing','violet'); INSERT INTO skill_repos VALUES('example','repo','feature/test',1); INSERT INTO skills VALUES('old-id','Display Demo','demo','example','repo','feature/test','https://github.com/example/repo/blob/feature/test/nested/demo/SKILL.md','group',0,1,'old-baseline');").unwrap();
    skill(&dir.join("skills/demo"), "local edit preserved");
    fs::create_dir_all(home.join(".codex/skills")).unwrap();
    symlink(
        "../../.cc-switch/skills/demo",
        home.join(".codex/skills/demo"),
    )
    .unwrap();
    dir
}
#[test]
fn deleted_history_stays_hidden_without_losing_undo_or_files() {
    let (_t, home, data, m) = fixture();
    let cc_dir = cc(&home);
    let p = m
        .prepare_cc(cc_dir.to_string_lossy().into(), "independent".into())
        .unwrap();
    m.apply_admin(&p.token).unwrap();
    let completed = data
        .join("admin-plans")
        .join(&p.token)
        .join("completed.json");
    let original = fs::read(&completed).unwrap();
    let config = m.config().unwrap();
    let files = skill_manager::global::safe_tree(&home.join(".skill-manager/skills/demo")).unwrap();
    m.admin_command("delete_history", serde_json::json!({"token": p.token}))
        .unwrap();
    let reopened = Manager::new(&home, &data).unwrap();
    assert!(!reopened
        .overview()
        .unwrap()
        .history
        .iter()
        .any(|h| h.token == p.token));
    reopened.clear_cache().unwrap();
    assert_eq!(fs::read(&completed).unwrap(), original);
    assert_eq!(reopened.config().unwrap(), config);
    assert_eq!(
        skill_manager::global::safe_tree(&home.join(".skill-manager/skills/demo")).unwrap(),
        files
    );
    reopened
        .admin_command("restore_history", serde_json::json!({"token": p.token}))
        .unwrap();
    assert!(reopened
        .overview()
        .unwrap()
        .history
        .iter()
        .any(|h| h.token == p.token));
    let undo = reopened.undo_admin(&p.token).unwrap();
    reopened.apply_admin(&undo.token).unwrap();
    assert!(cc_dir.join("skills/demo/SKILL.md").is_file());
}

#[test]
fn delete_history_rejects_unfinished_invalid_and_redirected_records() {
    let (_t, home, data, m) = fixture();
    let cc_dir = cc(&home);
    let p = m
        .prepare_cc(cc_dir.to_string_lossy().into(), "independent".into())
        .unwrap();
    assert!(m.set_history_deleted(&p.token, true).is_err());
    assert!(m.set_history_deleted("../outside", true).is_err());
    m.apply_admin(&p.token).unwrap();
    let dir = data.join("admin-plans").join(&p.token);
    let redirected = data.join("redirected-history");
    fs::rename(&dir, &redirected).unwrap();
    symlink(&redirected, &dir).unwrap();
    assert!(m.set_history_deleted(&p.token, true).is_err());
    assert!(!redirected.join("history-hidden.json").exists());
    assert!(redirected.join("completed.json").is_file());
}

#[test]
fn cc_migration_preserves_files_metadata_and_existing_links() {
    let (_t, home, _, m) = fixture();
    let cc = cc(&home);
    let db_before = fs::read(cc.join("cc-switch.db")).unwrap();
    let p = m
        .prepare_cc(cc.to_string_lossy().into(), "independent".into())
        .unwrap();
    assert!(cc.join("skills/demo").exists());
    assert!(!home.join(".skill-manager/skills/demo").exists());
    m.apply_admin(&p.token).unwrap();
    let i = m.inventory().unwrap();
    assert_eq!(i.skills.len(), 1);
    let s = &i.skills[0];
    assert!(s.source.contains(".skill-manager/skills"));
    assert_eq!(s.clients["Codex"], "link");
    assert_eq!(s.clients["Claude"], "off");
    assert_eq!(s.origin.as_ref().unwrap().path, "nested/demo");
    assert_eq!(s.origin.as_ref().unwrap().reference, "feature/test");
    assert!(s.local_modified);
    let config = m.config().unwrap();
    assert_eq!(config.groups.len(), 1);
    assert_eq!(config.repos.len(), 1);
    assert!(config.members.contains_key(&s.id));
    assert_eq!(config.legacy[&s.id]["content_hash"], "old-baseline");
    assert_eq!(db_before, fs::read(cc.join("cc-switch.db")).unwrap());
    let again = m
        .prepare_cc(cc.to_string_lossy().into(), "independent".into())
        .unwrap();
    assert!(again.moves.is_empty());
}
#[test]
fn cc_migration_has_full_undo() {
    let (_t, home, _, m) = fixture();
    let cc = cc(&home);
    let p = m
        .prepare_cc(cc.to_string_lossy().into(), "independent".into())
        .unwrap();
    m.apply_admin(&p.token).unwrap();
    let undo = m.undo_admin(&p.token).unwrap();
    m.apply_admin(&undo.token).unwrap();
    assert_eq!(
        fs::read_link(home.join(".codex/skills/demo")).unwrap(),
        Path::new("../../.cc-switch/skills/demo")
    );
    assert!(cc.join("skills/demo").exists());
    assert!(m.config().unwrap().imported.is_empty());
}
#[test]
fn cc_migration_preflight_refuses_destination_conflict() {
    let (_t, home, _, m) = fixture();
    let cc = cc(&home);
    skill(&home.join(".skill-manager/skills/demo"), "foreign");
    assert!(m
        .prepare_cc(cc.to_string_lossy().into(), "independent".into())
        .is_err());
    assert!(cc.join("skills/demo").exists());
}
#[test]
fn cc_metadata_drift_invalidates_preview() {
    let (_t, home, _, m) = fixture();
    let cc = cc(&home);
    let p = m
        .prepare_cc(cc.to_string_lossy().into(), "independent".into())
        .unwrap();
    Connection::open(cc.join("cc-switch.db"))
        .unwrap()
        .execute("UPDATE skills SET enabled_codex=0", [])
        .unwrap();
    assert!(m.apply_admin(&p.token).unwrap_err().contains("cc-switch"));
    assert!(cc.join("skills/demo").exists());
}
#[test]
fn every_migration_step_can_roll_back() {
    for step in 1..=3 {
        let (_t, home, _, m) = fixture();
        let cc = cc(&home);
        let p = m
            .prepare_cc(cc.to_string_lossy().into(), "independent".into())
            .unwrap();
        assert!(m.apply_admin_with_failure(&p.token, Some(step)).is_err());
        assert!(cc.join("skills/demo/SKILL.md").exists());
        assert!(home.join(".codex/skills/demo/SKILL.md").exists());
        assert!(!m.inventory().unwrap().pending);
    }
}
#[test]
fn migration_journal_recovers_after_restart() {
    let (_t, home, data, m) = fixture();
    let cc = cc(&home);
    let p = m
        .prepare_cc(cc.to_string_lossy().into(), "independent".into())
        .unwrap();
    for mv in &p.moves[..2] {
        fs::create_dir_all(mv.to.parent().unwrap()).unwrap();
        fs::rename(&mv.from, &mv.to).unwrap();
    }
    write(
        &data.join("admin-transaction.json"),
        &serde_json::json!({"plan":p,"completed":1,"in_flight":true}).to_string(),
    );
    drop(m);
    let m = Manager::new(&home, &data).unwrap();
    m.recover().unwrap();
    assert!(home.join(".codex/skills/demo/SKILL.md").exists());
    assert!(!m.inventory().unwrap().pending);
}
#[test]
fn changed_link_after_preview_stops_entire_migration() {
    let (_t, home, _, m) = fixture();
    let cc = cc(&home);
    let p = m
        .prepare_cc(cc.to_string_lossy().into(), "independent".into())
        .unwrap();
    let link = home.join(".codex/skills/demo");
    fs::remove_file(&link).unwrap();
    symlink("/foreign/skill", &link).unwrap();
    assert!(m.apply_admin(&p.token).is_err());
    assert!(cc.join("skills/demo").exists());
    assert_eq!(fs::read_link(link).unwrap(), Path::new("/foreign/skill"));
}
#[test]
fn groups_support_replace_move_rename_and_delete() {
    let (t, _, _, m) = fixture();
    let id = install(&t, &m);
    m.save_group(
        Some("one".into()),
        "One".into(),
        "blue".into(),
        Some(vec![id.clone()]),
    )
    .unwrap();
    m.save_group(Some("two".into()), "Two".into(), "rose".into(), None)
        .unwrap();
    m.move_group(vec![id.clone()], Some("two".into())).unwrap();
    assert_eq!(m.config().unwrap().members[&id], "two");
    m.save_group(
        Some("two".into()),
        "Renamed".into(),
        "cyan".into(),
        Some(vec![]),
    )
    .unwrap();
    assert!(m.config().unwrap().members.is_empty());
    m.delete_group("one").unwrap();
    assert_eq!(m.config().unwrap().groups.len(), 1);
    assert!(Path::new(&m.inventory().unwrap().skills[0].source).exists());
}
#[test]
fn group_order_persists_without_changing_members_and_survives_edits() {
    let (t, home, data, m) = fixture();
    let id = install(&t, &m);
    for (group, color) in [("one", "blue"), ("two", "rose"), ("three", "cyan")] {
        m.save_group(Some(group.into()), group.into(), color.into(), None)
            .unwrap();
    }
    m.move_group(vec![id.clone()], Some("two".into())).unwrap();
    let before = m.config().unwrap();
    m.admin_command(
        "reorder_groups",
        serde_json::json!({"ids": ["three", "one", "two"]}),
    )
    .unwrap();
    let reopened = Manager::new(&home, &data).unwrap();
    let mut expected = before.clone();
    expected.groups = vec![
        before.groups[2].clone(),
        before.groups[0].clone(),
        before.groups[1].clone(),
    ];
    assert_eq!(reopened.config().unwrap(), expected);
    reopened
        .save_group(
            Some("three".into()),
            "Renamed".into(),
            "violet".into(),
            None,
        )
        .unwrap();
    expected.groups[0].name = "Renamed".into();
    expected.groups[0].color = "violet".into();
    assert_eq!(reopened.config().unwrap(), expected);
    for invalid in [
        vec!["one", "two"],
        vec!["three", "one", "one"],
        vec!["three", "one", "foreign"],
        vec!["three", "one", "two", "two"],
        vec![],
    ] {
        assert!(reopened
            .reorder_groups(invalid.into_iter().map(String::from).collect())
            .is_err());
        assert_eq!(reopened.config().unwrap(), expected);
    }
    assert_eq!(reopened.config().unwrap().members[&id], "two");
}
#[test]
fn repositories_persist_and_validate() {
    let (_t, _, _, m) = fixture();
    let r = Repo {
        repo: "example/repo".into(),
        reference: "main".into(),
        enabled: true,
    };
    m.save_repo(r.clone(), false).unwrap();
    let mut updated = r.clone();
    updated.enabled = false;
    m.save_repo(updated, false).unwrap();
    assert_eq!(m.config().unwrap().repos.len(), 1);
    assert!(!m.config().unwrap().repos[0].enabled);
    m.save_repo(r, true).unwrap();
    assert!(m.config().unwrap().repos.is_empty());
}
#[test]
fn copy_mode_tracks_ownership_and_preserves_external_edits() {
    let (t, home, _, m) = fixture();
    let id = install(&t, &m);
    m.set_sync_method("copy".into()).unwrap();
    m.toggle(&id, vec!["Codex".into()], true).unwrap();
    let dest = home.join(".codex/skills/demo");
    assert!(!fs::symlink_metadata(&dest)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(m.inventory().unwrap().skills.len(), 1);
    assert_eq!(m.inventory().unwrap().skills[0].clients["Codex"], "copy");
    write(&dest.join("SKILL.md"), "changed");
    assert_eq!(
        m.inventory().unwrap().skills[0].clients["Codex"],
        "modified"
    );
    assert!(m.toggle(&id, vec!["Codex".into()], false).is_err());
    assert_eq!(
        fs::read_to_string(dest.join("SKILL.md")).unwrap(),
        "changed"
    );
}
#[test]
fn switch_copy_to_link_and_back_is_reversible() {
    let (t, home, _, m) = fixture();
    let id = install(&t, &m);
    m.set_sync_method("copy".into()).unwrap();
    m.toggle(&id, vec!["Codex".into()], true).unwrap();
    let p = m
        .prepare_sync(
            vec![id.clone()],
            vec!["Codex".into()],
            true,
            Some("symlink".into()),
        )
        .unwrap();
    m.apply_admin(&p.token).unwrap();
    assert!(fs::read_link(home.join(".codex/skills/demo")).is_ok());
    m.set_sync_method("copy".into()).unwrap();
    m.toggle(&id, vec!["Codex".into()], true).unwrap();
    assert!(fs::read_link(home.join(".codex/skills/demo")).is_err());
    m.toggle(&id, vec!["Codex".into()], false).unwrap();
    assert!(!home.join(".codex/skills/demo").exists());
}
#[test]
fn uninstall_and_restore_keeps_complete_backup() {
    let (t, home, _, m) = fixture();
    let id = install(&t, &m);
    m.toggle(&id, vec!["Codex".into()], true).unwrap();
    let p = m.prepare_uninstall(vec![id]).unwrap();
    m.apply_admin(&p.token).unwrap();
    assert!(m.inventory().unwrap().skills.is_empty());
    assert!(!home.join(".codex/skills/demo").exists());
    let b = m.overview().unwrap().backups;
    assert_eq!(b.len(), 1);
    let restore = m
        .prepare_restore_removed(&b[0].token, vec!["Codex".into()])
        .unwrap();
    m.apply_admin(&restore.token).unwrap();
    assert!(home.join(".codex/skills/demo/ref/a.md").exists());
    assert_eq!(m.overview().unwrap().backups.len(), 1);
}
#[test]
fn adopt_client_source_moves_it_to_library_and_replaces_with_link() {
    let (_t, home, _, m) = fixture();
    skill(&home.join(".codex/skills/demo"), "own");
    let id = m.inventory().unwrap().skills[0].id.clone();
    let p = m
        .prepare_storage("independent".into(), Some(vec![id]), true)
        .unwrap();
    m.apply_admin(&p.token).unwrap();
    assert_eq!(
        fs::read_link(home.join(".codex/skills/demo")).unwrap(),
        home.join(".skill-manager/skills/demo")
    );
}
#[test]
fn zip_import_supports_root_skill_and_multiple_nested_skills() {
    for root in [true, false] {
        let (_t, _, _, m) = fixture();
        let t = tempfile::tempdir().unwrap();
        let archive = t.path().join("test.skill");
        let mut w = zip::ZipWriter::new(Cursor::new(vec![]));
        let option = zip::write::SimpleFileOptions::default();
        let names = if root {
            vec!["SKILL.md"]
        } else {
            vec!["one/SKILL.md", "two/SKILL.md"]
        };
        for path in &names {
            w.start_file(path, option).unwrap();
            w.write_all(
                format!(
                    "---\nname: {}\n---\nhello",
                    path.split('/').next().unwrap().replace("SKILL.md", "root")
                )
                .as_bytes(),
            )
            .unwrap();
        }
        fs::write(&archive, w.finish().unwrap().into_inner()).unwrap();
        let d = m.inspect_archive(&archive).unwrap();
        assert_eq!(d.candidates.len(), names.len());
        let p = m
            .prepare_archive(
                &d.token,
                d.candidates.iter().map(|c| c.path.clone()).collect(),
            )
            .unwrap();
        m.apply_admin(&p.token).unwrap();
        assert_eq!(m.inventory().unwrap().skills.len(), names.len());
    }
}
#[test]
fn local_archive_rejects_traversal() {
    let mut w = zip::ZipWriter::new(Cursor::new(vec![]));
    w.start_file("../outside", zip::write::SimpleFileOptions::default())
        .unwrap();
    w.write_all(b"bad").unwrap();
    let t = tempfile::tempdir().unwrap();
    assert!(registry::extract_local_archive(&w.finish().unwrap().into_inner(), t.path()).is_err());
}
#[test]
fn copies_receive_upgrades_in_same_transaction() {
    let (t, home, data, m) = fixture();
    let id = install(&t, &m);
    m.set_sync_method("copy".into()).unwrap();
    m.toggle(&id, vec!["Codex".into()], true).unwrap();
    let root = data.join("discoveries/123");
    skill(&root.join("tree/demo"), "new");
    let d = Discovery {
        token: "123".into(),
        repo: "example/repo".into(),
        reference: "main".into(),
        candidates: vec![Candidate {
            path: "demo".into(),
            name: "demo".into(),
            description: "Demo".into(),
        }],
    };
    write(
        &root.join("discovery.json"),
        &serde_json::to_string(&d).unwrap(),
    );
    let p = m.prepare_remote("123", "demo", Some(&id)).unwrap();
    m.apply(&p.token, true).unwrap();
    assert!(fs::read_to_string(home.join(".codex/skills/demo/SKILL.md"))
        .unwrap()
        .contains("new"));
    assert_eq!(m.inventory().unwrap().skills[0].clients["Codex"], "copy");
}
#[test]
fn cc_share_link_import_preserves_repo_and_branch() {
    use skill_manager::global::admin::parse_repo_input;
    let r=parse_repo_input(Repo{repo:"ccswitch://v1/import?resource=skill&repo=example%2Frepo&branch=feature%2Ftest&enabled=false".into(),reference:"HEAD".into(),enabled:true}).unwrap();
    assert_eq!(r.repo, "example/repo");
    assert_eq!(r.reference, "feature/test");
    assert!(!r.enabled);
    assert!(parse_repo_input(Repo {
        repo: "ccswitch://v1/import?resource=provider&repo=example/repo".into(),
        reference: "HEAD".into(),
        enabled: true
    })
    .is_err());
}
#[test]
fn uninstall_preserves_modified_copy_as_unmanaged_resource() {
    let (t, home, _, m) = fixture();
    let id = install(&t, &m);
    m.set_sync_method("copy".into()).unwrap();
    m.toggle(&id, vec!["Codex".into()], true).unwrap();
    write(&home.join(".codex/skills/demo/SKILL.md"), "modified");
    let p = m.prepare_uninstall(vec![id]).unwrap();
    assert!(!p.warnings.is_empty());
    m.apply_admin(&p.token).unwrap();
    let i = m.inventory().unwrap();
    assert_eq!(i.skills.len(), 1);
    assert!(!i.skills[0].managed);
    assert_eq!(
        fs::read_to_string(home.join(".codex/skills/demo/SKILL.md")).unwrap(),
        "modified"
    );
}
#[test]
fn directory_redirection_after_preview_is_rejected() {
    let (t, home, _, m) = fixture();
    let id = install(&t, &m);
    let p = m
        .prepare_sync(vec![id], vec!["Codex".into()], true, None)
        .unwrap();
    let stage = p.moves[0].from.parent().unwrap();
    let other = t.path().join("other");
    fs::rename(stage, &other).unwrap();
    symlink(&other, stage).unwrap();
    assert!(m.apply_admin(&p.token).is_err());
    assert!(!home.join(".codex/skills/demo").exists());
}
#[test]
fn archive_root_without_name_uses_filename() {
    let (_t, _, _, m) = fixture();
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("root-demo.zip");
    let mut w = zip::ZipWriter::new(Cursor::new(vec![]));
    w.start_file("SKILL.md", zip::write::SimpleFileOptions::default())
        .unwrap();
    w.write_all(b"# Root skill").unwrap();
    fs::write(&p, w.finish().unwrap().into_inner()).unwrap();
    let d = m.inspect_archive(&p).unwrap();
    assert_eq!(d.candidates[0].name, "root-demo");
}
#[test]
fn cc_backup_import_keeps_content_origin_and_source_backup() {
    let (_t, home, _, m) = fixture();
    let cc = cc(&home);
    let backup = cc.join("skill-backups/old");
    skill(&backup.join("skill"), "historic");
    let meta = serde_json::json!({"backupCreatedAt":123,"skill":{"directory":"historic","repoOwner":"example","repoName":"repo","repoBranch":"main","readmeUrl":"https://github.com/example/repo/blob/main/nested/historic/SKILL.md","apps":{"codex":true}}});
    write(&backup.join("meta.json"), &meta.to_string());
    let p = m
        .prepare_cc(cc.to_string_lossy().into(), "independent".into())
        .unwrap();
    m.apply_admin(&p.token).unwrap();
    let b = m.overview().unwrap().backups;
    assert_eq!(b.len(), 1);
    assert_eq!(
        b[0].previous.origin.as_ref().unwrap().path,
        "nested/historic"
    );
    assert!(b[0].previous.expected.contains("Codex"));
    assert_eq!(b[0].created, 123);
    assert!(backup.join("skill/SKILL.md").exists());
}
#[test]
fn storage_migration_keeps_prior_upgrade_backups_attached() {
    let (t, _, data, m) = fixture();
    let id = install(&t, &m);
    let root = data.join("discoveries/123");
    skill(&root.join("tree/demo"), "updated");
    let d = Discovery {
        token: "123".into(),
        repo: "example/repo".into(),
        reference: "main".into(),
        candidates: vec![Candidate {
            path: "demo".into(),
            name: "demo".into(),
            description: "".into(),
        }],
    };
    write(
        &root.join("discovery.json"),
        &serde_json::to_string(&d).unwrap(),
    );
    let upgrade = m.prepare_remote("123", "demo", Some(&id)).unwrap();
    m.apply(&upgrade.token, true).unwrap();
    let storage = m
        .prepare_storage("independent".into(), None, false)
        .unwrap();
    m.apply_admin(&storage.token).unwrap();
    let new_id = m.inventory().unwrap().skills[0].id.clone();
    assert_ne!(new_id, id);
    let backups = m.backups(&new_id).unwrap();
    assert_eq!(backups.len(), 1);
    let restore = m.prepare_restore(&new_id, &backups[0].token).unwrap();
    m.apply(&restore.token, true).unwrap();
    assert!(m.details(&new_id).unwrap().contains("one"));
}

fn duplicate_choices(m: &Manager) -> serde_json::Value {
    let report = m
        .admin_command("duplicates", serde_json::json!({}))
        .unwrap();
    let sources = report["groups"][0]["sources"].as_array().unwrap();
    serde_json::json!({"choices": [{"keepId": sources[0]["id"], "duplicateIds": sources[1..].iter().map(|s| s["id"].clone()).collect::<Vec<_>>()}]})
}
fn duplicate_plan(m: &Manager) -> skill_manager::global::admin::AdminPlan {
    serde_json::from_value(
        m.admin_command("duplicates_preview", duplicate_choices(m))
            .unwrap(),
    )
    .unwrap()
}
#[test]
fn duplicate_scan_collapses_aliases_and_compares_the_whole_tree() {
    let (_t, home, _, m) = fixture();
    let source = home.join(".cc-switch/skills/demo");
    let other = home.join(".agents/skills/demo");
    skill(&source, "one");
    skill(&other, "one");
    fs::create_dir_all(home.join(".codex/skills")).unwrap();
    symlink(&source, home.join(".codex/skills/demo")).unwrap();
    let report = m
        .admin_command("duplicates", serde_json::json!({}))
        .unwrap();
    assert_eq!(report["groups"].as_array().unwrap().len(), 1);
    let sources = report["groups"][0]["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0]["source"], source.to_str().unwrap());
    assert_eq!(sources[0]["digest"], sources[1]["digest"]);
    write(&other.join("ref/a.md"), "different supporting instructions");
    assert!(m
        .admin_command("duplicates_preview", duplicate_choices(&m))
        .is_err());
    assert!(!other.is_symlink());
}
#[test]
fn duplicate_batch_backs_up_links_merges_inventory_and_can_undo() {
    let (_t, home, _, m) = fixture();
    let source = home.join(".cc-switch/skills/demo");
    let a = home.join(".agents/skills/demo");
    let b = home.join(".codex/skills/demo");
    for p in [&source, &a, &b] {
        skill(p, "one");
    }
    let before = skill_manager::global::safe_tree(&a).unwrap();
    let p = duplicate_plan(&m);
    assert_eq!(p.moves.len(), 4);
    assert!(!a.is_symlink()); // preview never touches originals
    m.apply_admin(&p.token).unwrap();
    assert_eq!(m.inventory().unwrap().skills.len(), 1);
    assert_eq!(fs::read_link(&a).unwrap(), source);
    assert_eq!(fs::read_link(&b).unwrap(), source);
    assert_eq!(
        skill_manager::global::safe_tree(&p.moves[0].to).unwrap(),
        before
    );
    assert!(m
        .overview()
        .unwrap()
        .history
        .iter()
        .any(|h| h.kind == "deduplicate"));
    m.clear_cache().unwrap(); // completed backups survive cache cleanup
    let undo = m.undo_admin(&p.token).unwrap();
    m.apply_admin(&undo.token).unwrap();
    assert!(!a.is_symlink() && !b.is_symlink());
    assert_eq!(skill_manager::global::safe_tree(&a).unwrap(), before);
    assert_eq!(m.inventory().unwrap().skills.len(), 3);
}
#[test]
fn duplicate_failure_at_each_move_rolls_back_files_and_metadata() {
    for at in 1..=4 {
        let (_t, home, _, m) = fixture();
        for root in [".cc-switch", ".agents", ".codex"] {
            skill(&home.join(root).join("skills/demo"), "one");
        }
        let p = duplicate_plan(&m);
        assert!(m.apply_admin_with_failure(&p.token, Some(at)).is_err());
        for root in [".cc-switch", ".agents", ".codex"] {
            assert!(!home.join(root).join("skills/demo").is_symlink());
        }
        assert_eq!(m.inventory().unwrap().skills.len(), 3);
        assert!(!m.inventory().unwrap().pending);
        assert!(m.config().unwrap().aliases.is_empty());
    }
}
#[test]
fn duplicate_apply_refuses_changed_source_copy_or_permissions() {
    use std::os::unix::fs::PermissionsExt;
    for case in 0..3 {
        let (_t, home, _, m) = fixture();
        let a = home.join(".cc-switch/skills/demo");
        let b = home.join(".agents/skills/demo");
        skill(&a, "one");
        skill(&b, "one");
        let p = duplicate_plan(&m);
        match case {
            0 => write(&a.join("ref/a.md"), "changed"),
            1 => write(&b.join("ref/a.md"), "changed"),
            _ => {
                fs::set_permissions(b.join("ref/a.md"), fs::Permissions::from_mode(0o600)).unwrap()
            }
        }
        assert!(m.apply_admin(&p.token).is_err());
        assert!(!a.is_symlink() && !b.is_symlink());
    }
}
#[test]
fn duplicate_preview_rejects_incompatible_permissions_unsafe_trees_and_bad_ids() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, home, _, m) = fixture();
    let a = home.join(".cc-switch/skills/demo");
    let b = home.join(".agents/skills/demo");
    skill(&a, "one");
    skill(&b, "one");
    fs::set_permissions(b.join("ref/a.md"), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(m
        .admin_command("duplicates_preview", duplicate_choices(&m))
        .is_err());
    fs::set_permissions(
        b.join("ref/a.md"),
        fs::metadata(a.join("ref/a.md")).unwrap().permissions(),
    )
    .unwrap();
    symlink(&a, b.join("external")).unwrap();
    assert!(m
        .admin_command("duplicates_preview", duplicate_choices(&m))
        .is_err());
    assert!(m
        .admin_command(
            "duplicates_preview",
            serde_json::json!({"choices": [{"keepId": "../../outside", "duplicateIds": ["bad"]}]})
        )
        .is_err());
}
#[test]
fn duplicate_merging_preserves_managed_record_group_and_client_expectations() {
    let (t, home, _, m) = fixture();
    let id = install(&t, &m);
    m.toggle(&id, vec!["Codex".into()], true).unwrap();
    m.save_group(None, "Team".into(), "blue".into(), Some(vec![id.clone()]))
        .unwrap();
    let other = home.join(".agents/skills/demo");
    skill(&other, "one");
    let report = m
        .admin_command("duplicates", serde_json::json!({}))
        .unwrap();
    let other_id = report["groups"][0]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["source"] == other.to_str().unwrap())
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let p: skill_manager::global::admin::AdminPlan = serde_json::from_value(
        m.admin_command(
            "duplicates_preview",
            serde_json::json!({"choices": [{"keepId": other_id, "duplicateIds": [id]}]}),
        )
        .unwrap(),
    )
    .unwrap();
    m.apply_admin(&p.token).unwrap();
    let inventory = m.inventory().unwrap();
    assert_eq!(inventory.skills.len(), 1);
    assert!(inventory.skills[0].managed);
    assert_eq!(inventory.skills[0].clients["Codex"], "link");
    assert!(inventory.skills[0].drift.is_empty());
    assert!(m.config().unwrap().members.contains_key(other_id));
}
#[test]
fn duplicate_undo_preserves_a_new_foreign_replacement() {
    let (_t, home, _, m) = fixture();
    let a = home.join(".cc-switch/skills/demo");
    let b = home.join(".agents/skills/demo");
    skill(&a, "one");
    skill(&b, "one");
    let p = duplicate_plan(&m);
    m.apply_admin(&p.token).unwrap();
    fs::remove_file(&b).unwrap();
    skill(&b, "new foreign content");
    let undo = m.undo_admin(&p.token).unwrap();
    assert!(m.apply_admin(&undo.token).is_err());
    assert!(fs::read_to_string(b.join("SKILL.md"))
        .unwrap()
        .contains("new foreign content"));
}

#[test]
fn duplicate_report_explains_file_and_root_permission_differences() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, home, _, m) = fixture();
    let a = home.join(".cc-switch/skills/demo");
    let b = home.join(".agents/skills/demo");
    skill(&a, "one");
    skill(&b, "two");
    fs::set_permissions(&a, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&b, fs::Permissions::from_mode(0o700)).unwrap();
    let report = m
        .admin_command("duplicates", serde_json::json!({}))
        .unwrap();
    let sources = report["groups"][0]["sources"].as_array().unwrap();
    assert_ne!(
        sources[0]["tree"]["SKILL.md"],
        sources[1]["tree"]["SKILL.md"]
    );
    assert_eq!(sources[0]["permissions"]["/"], 0o755);
    assert_eq!(sources[1]["permissions"]["/"], 0o700);
}
#[test]
fn explicit_version_choice_backs_up_different_content_and_permissions_and_undoes() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, home, _, m) = fixture();
    let a = home.join(".cc-switch/skills/demo");
    let b = home.join(".agents/skills/demo");
    skill(&a, "chosen");
    skill(&b, "other");
    fs::set_permissions(&b, fs::Permissions::from_mode(0o700)).unwrap();
    let mut args = duplicate_choices(&m);
    assert!(m.admin_command("duplicates_preview", args.clone()).is_err());
    args["choices"][0]["allowDifferent"] = true.into();
    let p: skill_manager::global::admin::AdminPlan =
        serde_json::from_value(m.admin_command("duplicates_preview", args).unwrap()).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("统一使用")));
    m.apply_admin(&p.token).unwrap();
    assert!(fs::read_to_string(b.join("SKILL.md"))
        .unwrap()
        .contains("chosen"));
    assert!(fs::read_to_string(p.moves[0].to.join("SKILL.md"))
        .unwrap()
        .contains("other"));
    let undo = m.undo_admin(&p.token).unwrap();
    m.apply_admin(&undo.token).unwrap();
    assert!(fs::read_to_string(b.join("SKILL.md"))
        .unwrap()
        .contains("other"));
    assert_eq!(
        fs::metadata(&b).unwrap().permissions().mode() & 0o777,
        0o700
    );
}
#[test]
fn unified_version_redirects_owned_client_copies_without_inheriting_old_origin() {
    let (t, home, _, m) = fixture();
    let id = install(&t, &m);
    let sync = m
        .prepare_sync(
            vec![id.clone()],
            vec!["Codex".into()],
            true,
            Some("copy".into()),
        )
        .unwrap();
    m.apply_admin(&sync.token).unwrap();
    let other = home.join(".agents/skills/demo");
    skill(&other, "chosen version");
    let other_id = m
        .inventory()
        .unwrap()
        .skills
        .into_iter()
        .find(|s| s.source == other.to_str().unwrap())
        .unwrap()
        .id;
    let p: skill_manager::global::admin::AdminPlan = serde_json::from_value(m.admin_command("duplicates_preview", serde_json::json!({"choices": [{"keepId": other_id, "duplicateIds": [id], "allowDifferent": true}]})).unwrap()).unwrap();
    m.apply_admin(&p.token).unwrap();
    let client = home.join(".codex/skills/demo");
    assert!(client.is_symlink());
    assert!(fs::read_to_string(client.join("SKILL.md"))
        .unwrap()
        .contains("chosen version"));
    let inv = m.inventory().unwrap();
    assert_eq!(inv.skills.len(), 1);
    assert!(!inv.skills[0].local_modified);
    assert!(m.config().unwrap().copies.is_empty());
    let undo = m.undo_admin(&p.token).unwrap();
    m.apply_admin(&undo.token).unwrap();
    assert!(!client.is_symlink());
    assert!(fs::read_to_string(client.join("SKILL.md"))
        .unwrap()
        .contains("one"));
}
#[test]
fn explicit_version_choice_still_refuses_stale_files_and_unreadable_trees() {
    let (_t, home, _, m) = fixture();
    let a = home.join(".cc-switch/skills/demo");
    let b = home.join(".agents/skills/demo");
    skill(&a, "one");
    skill(&b, "two");
    let mut args = duplicate_choices(&m);
    args["choices"][0]["allowDifferent"] = true.into();
    let p: skill_manager::global::admin::AdminPlan =
        serde_json::from_value(m.admin_command("duplicates_preview", args.clone()).unwrap())
            .unwrap();
    write(&b.join("ref/a.md"), "new edit");
    assert!(m.apply_admin(&p.token).is_err());
    symlink(&a, b.join("unsafe")).unwrap();
    assert!(m.admin_command("duplicates_preview", args).is_err());
    assert!(!b.is_symlink());
}

fn compare_args(m: &Manager, path: Option<&str>) -> serde_json::Value {
    let report = m
        .admin_command("duplicates", serde_json::json!({}))
        .unwrap();
    let sources = report["groups"][0]["sources"].as_array().unwrap();
    serde_json::json!({"leftId": sources[0]["id"], "rightId": sources[1]["id"], "path": path})
}
#[test]
fn skill_diff_reads_supporting_files_and_reports_added_removed_and_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, home, data, m) = fixture();
    let a = home.join(".cc-switch/skills/demo");
    let b = home.join(".agents/skills/demo");
    skill(&a, "old");
    skill(&b, "new");
    write(&a.join("left-only.md"), "only left\n");
    write(&b.join("right-only.md"), "only right\n");
    fs::set_permissions(&a, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&b, fs::Permissions::from_mode(0o700)).unwrap();
    let result = m
        .admin_command("duplicate_compare", compare_args(&m, Some("ref/a.md")))
        .unwrap();
    let patch = result["patch"].as_str().unwrap();
    assert!(patch.contains("-old") && patch.contains("+new"));
    let files = result["files"].as_array().unwrap();
    assert!(files
        .iter()
        .any(|f| f["path"] == "left-only.md" && f["status"] == "removed"));
    assert!(files
        .iter()
        .any(|f| f["path"] == "right-only.md" && f["status"] == "added"));
    assert!(files
        .iter()
        .any(|f| f["path"] == "" && f["status"] == "permissions"));
    let added = m
        .admin_command("duplicate_compare", compare_args(&m, Some("right-only.md")))
        .unwrap();
    assert!(added["left"].is_null());
    assert!(added["patch"].as_str().unwrap().contains("+only right"));
    assert!(!data.join("admin-plans").exists());
    assert!(!a.is_symlink() && !b.is_symlink());
}
#[test]
fn skill_diff_supports_any_pair_of_three_versions_and_preserves_newline_markers() {
    let (_t, home, _, m) = fixture();
    for (root, text) in [
        (".cc-switch", "第一版\n"),
        (".agents", "第二版"),
        (".claude", "第三版\r\n"),
    ] {
        skill(&home.join(root).join("skills/demo"), text);
    }
    let report = m
        .admin_command("duplicates", serde_json::json!({}))
        .unwrap();
    let sources = report["groups"][0]["sources"].as_array().unwrap();
    let result = m.admin_command("duplicate_compare", serde_json::json!({"leftId":sources[1]["id"],"rightId":sources[2]["id"],"path":"ref/a.md"})).unwrap();
    assert!(result["patch"].as_str().unwrap().contains("-第二版"));
    assert!(result["patch"].as_str().unwrap().contains("+第三版"));
    assert!(result["patch"].as_str().unwrap().contains("No newline"));
    assert_eq!(result["right"]["lineEndings"], "CRLF");
}
#[test]
fn skill_diff_explains_binary_large_and_permission_only_files() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, home, _, m) = fixture();
    let a = home.join(".cc-switch/skills/demo");
    let b = home.join(".agents/skills/demo");
    skill(&a, "same");
    skill(&b, "same");
    fs::write(a.join("binary"), [0, 1, 2]).unwrap();
    fs::write(b.join("binary"), [0, 1, 3]).unwrap();
    let result = m
        .admin_command("duplicate_compare", compare_args(&m, Some("binary")))
        .unwrap();
    assert!(result["left"]["notice"]
        .as_str()
        .unwrap()
        .contains("二进制"));
    assert_eq!(result["patch"], "");
    fs::write(a.join("large"), vec![b'a'; 1024 * 1024 + 1]).unwrap();
    let large = m
        .admin_command("duplicate_compare", compare_args(&m, Some("large")))
        .unwrap();
    assert!(large["left"]["notice"].as_str().unwrap().contains("1 MB"));
    fs::set_permissions(&a, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&b, fs::Permissions::from_mode(0o700)).unwrap();
    let root = m
        .admin_command("duplicate_compare", compare_args(&m, Some("")))
        .unwrap();
    assert_eq!(root["left"]["permissions"], 0o755);
    assert_eq!(root["right"]["permissions"], 0o700);
}
#[test]
fn skill_diff_rejects_traversal_unknown_ids_and_escaping_symlinks() {
    let (_t, home, _, m) = fixture();
    let a = home.join(".cc-switch/skills/demo");
    let b = home.join(".agents/skills/demo");
    skill(&a, "one");
    skill(&b, "two");
    for path in ["../secret", "/etc/passwd", "unknown", "ref/../SKILL.md"] {
        assert!(m
            .admin_command("duplicate_compare", compare_args(&m, Some(path)))
            .is_err());
    }
    let mut args = compare_args(&m, None);
    args["leftId"] = "bad".into();
    assert!(m.admin_command("duplicate_compare", args).is_err());
    let args = compare_args(&m, Some("escape"));
    symlink(&a, b.join("escape")).unwrap();
    assert!(m.admin_command("duplicate_compare", args).is_err());
}
#[test]
fn skill_diff_compares_internal_symlink_targets_without_following_them() {
    let (_t, home, _, m) = fixture();
    let a = home.join(".cc-switch/skills/demo");
    let b = home.join(".agents/skills/demo");
    for p in [&a, &b] {
        skill(p, "same");
        write(&p.join("other.md"), "same");
    }
    symlink("SKILL.md", a.join("alias")).unwrap();
    symlink("other.md", b.join("alias")).unwrap();
    let result = m
        .admin_command("duplicate_compare", compare_args(&m, Some("alias")))
        .unwrap();
    assert_eq!(result["left"]["kind"], "symlink");
    assert!(result["patch"].as_str().unwrap().contains("-SKILL.md"));
    assert!(result["patch"].as_str().unwrap().contains("+other.md"));
}

fn broken(home: &Path, client: &str, name: &str) -> PathBuf {
    let path = home.join(client).join("skills").join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    symlink(format!("../../gone/{name}"), &path).unwrap();
    path
}

#[test]
fn repair_links_matches_names_across_roots_and_restores_original_links() {
    let (_t, home, _, m) = fixture();
    let root = home.join(".skill-manager/skills");
    skill(&root.join("demo"), "chosen version");
    let a = broken(&home, ".agents", "demo");
    let b = broken(&home, ".codex", "demo");
    let missing = broken(&home, ".agents", "unknown");
    let valid = home.join(".codex/skills/valid");
    symlink(root.join("demo"), &valid).unwrap();
    // Alias roots must not duplicate the same link in the report.
    symlink(home.join(".agents"), home.join(".claude")).unwrap();
    let report = m.broken_links("~/.skill-manager/skills").unwrap();
    assert_eq!(report.links.len(), 3);
    assert_eq!(
        report.links.iter().filter(|l| l.target.is_some()).count(),
        2
    );
    assert!(report
        .links
        .iter()
        .find(|l| l.name == "unknown")
        .unwrap()
        .problem
        .is_some());
    let paths = vec![a.to_string_lossy().into(), b.to_string_lossy().into()];
    let plan = m
        .prepare_link_repair(root.to_str().unwrap(), paths)
        .unwrap();
    assert_eq!(plan.moves.len(), 4);
    assert!(!a.exists()); // Preview does not mutate the link.
    m.apply_admin(&plan.token).unwrap();
    assert_eq!(fs::read_link(&a).unwrap(), root.join("demo"));
    assert_eq!(fs::read_link(&b).unwrap(), root.join("demo"));
    assert!(!missing.exists());
    assert_eq!(m.overview().unwrap().history[0].kind, "repair-links");
    let undo = m.undo_admin(&plan.token).unwrap();
    m.apply_admin(&undo.token).unwrap();
    assert_eq!(fs::read_link(&a).unwrap(), Path::new("../../gone/demo"));
    assert_eq!(fs::read_link(&b).unwrap(), Path::new("../../gone/demo"));
    assert!(!a.exists());
    assert!(root.join("demo/SKILL.md").is_file());
}

#[test]
fn repair_links_only_changes_selected_links_and_resolves_target_aliases() {
    let (_t, home, _, m) = fixture();
    let target = home.join("real/demo");
    skill(&target, "source");
    let root = home.join("chosen");
    fs::create_dir_all(&root).unwrap();
    symlink(&target, root.join("demo")).unwrap();
    let a = broken(&home, ".agents", "demo");
    let b = broken(&home, ".codex", "demo");
    let p = m
        .prepare_link_repair(root.to_str().unwrap(), vec![a.to_string_lossy().into()])
        .unwrap();
    m.apply_admin(&p.token).unwrap();
    assert_eq!(fs::read_link(a).unwrap(), target);
    assert!(!b.exists());
}

#[test]
fn repair_links_rejects_unknown_paths_and_invalid_candidates() {
    let (_t, home, _, m) = fixture();
    let root = home.join("chosen");
    let a = broken(&home, ".agents", "demo");
    let report = m.broken_links(root.to_str().unwrap()).unwrap();
    assert!(report.links[0]
        .problem
        .as_ref()
        .unwrap()
        .contains("目标技能目录"));
    fs::create_dir_all(root.join("demo")).unwrap();
    assert!(m.broken_links(root.to_str().unwrap()).unwrap().links[0]
        .problem
        .as_ref()
        .unwrap()
        .contains("SKILL.md"));
    assert!(m
        .prepare_link_repair(root.to_str().unwrap(), vec![a.to_string_lossy().into()])
        .is_err());
    skill(&root.join("demo"), "source");
    let foreign = broken(&home, "unscanned", "demo");
    assert!(m
        .prepare_link_repair(
            root.to_str().unwrap(),
            vec![foreign.to_string_lossy().into()]
        )
        .is_err());
    assert!(m
        .prepare_link_repair(
            root.to_str().unwrap(),
            vec![a.to_string_lossy().into(), a.to_string_lossy().into()]
        )
        .is_err());
    symlink(home.join("private"), root.join("demo/escape")).unwrap();
    assert!(m.broken_links(root.to_str().unwrap()).unwrap().links[0]
        .target
        .is_none());
    assert!(!a.exists());
}

#[test]
fn repair_links_rechecks_links_targets_and_recovered_old_destinations() {
    for change in ["link", "content", "recovered", "target-alias", "directory"] {
        let (_t, home, _, m) = fixture();
        let root = home.join("chosen");
        skill(&root.join("demo"), "source");
        let a = broken(&home, ".agents", "demo");
        let p = m
            .prepare_link_repair(root.to_str().unwrap(), vec![a.to_string_lossy().into()])
            .unwrap();
        match change {
            "link" => {
                fs::remove_file(&a).unwrap();
                symlink("different-missing", &a).unwrap();
            }
            "content" => {
                write(&root.join("demo/ref/a.md"), "edited");
            }
            "recovered" => {
                skill(&home.join("gone/demo"), "restored original");
            }
            "target-alias" => {
                fs::rename(&root, home.join("moved")).unwrap();
                symlink(home.join("moved"), &root).unwrap();
            }
            "directory" => {
                fs::remove_file(&a).unwrap();
                skill(&a, "new real directory");
            }
            _ => unreachable!(),
        }
        assert!(m.apply_admin(&p.token).is_err(), "{change}");
        assert!(!m
            .overview()
            .unwrap()
            .history
            .iter()
            .any(|h| h.token == p.token));
        if change == "recovered" {
            assert_eq!(fs::read_link(&a).unwrap(), Path::new("../../gone/demo"));
        }
        if change == "directory" {
            assert!(fs::symlink_metadata(&a).unwrap().is_dir());
        }
    }
}

#[test]
fn repair_links_rolls_back_each_file_move_in_batch() {
    for fail in 1..=4 {
        let (_t, home, _, m) = fixture();
        let root = home.join("chosen");
        skill(&root.join("demo"), "source");
        let a = broken(&home, ".agents", "demo");
        let b = broken(&home, ".codex", "demo");
        let p = m
            .prepare_link_repair(
                root.to_str().unwrap(),
                vec![a.to_string_lossy().into(), b.to_string_lossy().into()],
            )
            .unwrap();
        assert!(m.apply_admin_with_failure(&p.token, Some(fail)).is_err());
        assert_eq!(fs::read_link(&a).unwrap(), Path::new("../../gone/demo"));
        assert_eq!(fs::read_link(&b).unwrap(), Path::new("../../gone/demo"));
        assert!(!a.exists());
        assert!(root.join("demo/SKILL.md").is_file());
    }
}

#[test]
fn repair_links_preserves_managed_metadata_and_updates_client_expectations() {
    let (t, home, _, m) = fixture();
    let id = install(&t, &m);
    let source = m
        .inventory()
        .unwrap()
        .skills
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .source
        .clone();
    let a = broken(&home, ".codex", "demo");
    let p = m
        .prepare_link_repair(
            Path::new(&source).parent().unwrap().to_str().unwrap(),
            vec![a.to_string_lossy().into()],
        )
        .unwrap();
    assert!(p.after.records[&id].expected.contains("Codex"));
    assert_eq!(p.before.records[&id].origin, p.after.records[&id].origin);
    assert_eq!(
        p.before.records[&id].baseline,
        p.after.records[&id].baseline
    );
    m.apply_admin(&p.token).unwrap();
    assert_eq!(
        m.inventory()
            .unwrap()
            .skills
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .clients["Codex"],
        "link"
    );
    // Undo must not overwrite a link manually changed after the repair.
    fs::remove_file(&a).unwrap();
    symlink("foreign-new-target", &a).unwrap();
    let undo = m.undo_admin(&p.token).unwrap();
    assert!(m.apply_admin(&undo.token).is_err());
    assert_eq!(fs::read_link(a).unwrap(), Path::new("foreign-new-target"));
}

#[test]
fn group_client_sync_is_atomic_and_does_not_touch_other_clients_or_sources() {
    let (t, home, _data, m) = fixture();
    let mut ids = Vec::new();
    for name in ["alpha", "beta"] {
        let source = t.path().join(name);
        write(
            &source.join("SKILL.md"),
            &format!("---\nname: {name}\ndescription: Test\n---\nbody"),
        );
        let p = m.prepare_local(&source).unwrap();
        m.apply(&p.token, false).unwrap();
        ids.push(p.record.id);
    }
    let setup = m
        .prepare_sync(
            ids.clone(),
            vec!["Claude".into(), "Codex".into()],
            true,
            None,
        )
        .unwrap();
    m.apply_admin(&setup.token).unwrap();
    let source = home.join(".skill-manager/skills/alpha/SKILL.md");
    // The source is discovered from inventory because library defaults can vary.
    let source = if source.exists() {
        source
    } else {
        PathBuf::from(
            &m.inventory()
                .unwrap()
                .skills
                .iter()
                .find(|s| s.id == ids[0])
                .unwrap()
                .source,
        )
        .join("SKILL.md")
    };
    let before = fs::read(&source).unwrap();
    let plan = m
        .prepare_sync(ids.clone(), vec!["Codex".into()], false, None)
        .unwrap();
    assert!(m.apply_admin_with_failure(&plan.token, Some(1)).is_err());
    assert!(home.join(".codex/skills/alpha/SKILL.md").exists());
    assert!(home.join(".codex/skills/beta/SKILL.md").exists());
    let plan = m
        .prepare_sync(ids, vec!["Codex".into()], false, None)
        .unwrap();
    m.apply_admin(&plan.token).unwrap();
    assert!(!home.join(".codex/skills/alpha").exists());
    assert!(!home.join(".codex/skills/beta").exists());
    assert!(home.join(".claude/skills/alpha/SKILL.md").exists());
    assert!(home.join(".claude/skills/beta/SKILL.md").exists());
    assert_eq!(before, fs::read(source).unwrap());
}

#[test]
fn preset_mixed_client_changes_share_a_transaction_and_roll_back_together() {
    use skill_manager::global::admin::PresetSyncChange;
    let (t, home, _data, m) = fixture();
    let mut ids = Vec::new();
    for name in ["alpha", "beta"] {
        let source = t.path().join(name);
        write(
            &source.join("SKILL.md"),
            &format!("---\nname: {name}\ndescription: Test\n---\nbody"),
        );
        let p = m.prepare_local(&source).unwrap();
        m.apply(&p.token, false).unwrap();
        ids.push(p.record.id);
    }
    let setup = m
        .prepare_sync(ids.clone(), vec!["Claude".into()], true, None)
        .unwrap();
    m.apply_admin(&setup.token).unwrap();
    let setup = m
        .prepare_sync(vec![ids[0].clone()], vec!["Codex".into()], true, None)
        .unwrap();
    m.apply_admin(&setup.token).unwrap();
    let changes = || {
        vec![
            PresetSyncChange {
                id: ids[0].clone(),
                client: "Codex".into(),
                enable: false,
            },
            PresetSyncChange {
                id: ids[1].clone(),
                client: "Codex".into(),
                enable: true,
            },
        ]
    };
    let p = m.prepare_preset_sync(changes()).unwrap();
    assert!(m.apply_admin_with_failure(&p.token, Some(2)).is_err());
    assert!(home.join(".codex/skills/alpha/SKILL.md").exists());
    assert!(!home.join(".codex/skills/beta").exists());
    let p = m.prepare_preset_sync(changes()).unwrap();
    m.apply_admin(&p.token).unwrap();
    assert!(!home.join(".codex/skills/alpha").exists());
    assert!(home.join(".codex/skills/beta/SKILL.md").exists());
    assert!(home.join(".claude/skills/alpha/SKILL.md").exists());
    assert!(home.join(".claude/skills/beta/SKILL.md").exists());
}

fn update_diff_plan(
    t: &tempfile::TempDir,
    data: &Path,
    m: &Manager,
) -> skill_manager::global::Plan {
    let id = install(t, m);
    let dir = data.join("discoveries/123");
    skill(&dir.join("tree/demo"), "新版本\n");
    write(&dir.join("tree/demo/added.md"), "新增文件\n");
    fs::write(dir.join("tree/demo/binary.bin"), [0, 1, 2]).unwrap();
    let discovery = Discovery {
        token: "123".into(),
        repo: "example/repo".into(),
        reference: "HEAD".into(),
        candidates: vec![Candidate {
            path: "demo".into(),
            name: "demo".into(),
            description: "Demo".into(),
        }],
    };
    write(
        &dir.join("discovery.json"),
        &serde_json::to_string(&discovery).unwrap(),
    );
    m.prepare_remote("123", "demo", Some(&id)).unwrap()
}
#[test]
fn update_plan_diff_compares_payload_supporting_files_and_keeps_sources_untouched() {
    let (t, _, data, m) = fixture();
    let plan = update_diff_plan(&t, &data, &m);
    let plans_before = fs::read_dir(data.join("admin-plans"))
        .map(|entries| entries.count())
        .unwrap_or(0);
    for path in ["SKILL.md", "ref/a.md"] {
        let diff = m
            .admin_command(
                "plan_compare",
                serde_json::json!({"token":plan.token,"path":path}),
            )
            .unwrap();
        let patch = diff["patch"].as_str().unwrap();
        assert!(patch.contains("-one") && patch.contains("+新版本"));
        assert!(patch.contains("No newline at end of file"));
    }
    let added = m
        .admin_command(
            "plan_compare",
            serde_json::json!({"token":plan.token,"path":"added.md"}),
        )
        .unwrap();
    assert!(added["left"].is_null());
    assert!(added["patch"].as_str().unwrap().contains("+新增文件"));
    let binary = m
        .admin_command(
            "plan_compare",
            serde_json::json!({"token":plan.token,"path":"binary.bin"}),
        )
        .unwrap();
    assert!(binary["notice"]
        .as_str()
        .unwrap()
        .contains("无法生成文本 Diff"));
    assert!(
        fs::read_to_string(Path::new(&plan.record.source).join("SKILL.md"))
            .unwrap()
            .ends_with("one")
    );
    assert_eq!(
        fs::read_dir(data.join("admin-plans"))
            .map(|entries| entries.count())
            .unwrap_or(0),
        plans_before
    );
    m.apply(&plan.token, true).unwrap();
    assert!(m
        .admin_command(
            "plan_compare",
            serde_json::json!({"token":plan.token,"path":"SKILL.md"})
        )
        .is_err());
}
#[test]
fn update_plan_diff_rejects_stale_files_payload_tampering_and_invalid_paths() {
    let (t, _, data, m) = fixture();
    let plan = update_diff_plan(&t, &data, &m);
    for (token, path) in [
        ("../outside", "SKILL.md"),
        (&*plan.token, "../outside"),
        (&*plan.token, "missing.md"),
    ] {
        assert!(m
            .admin_command(
                "plan_compare",
                serde_json::json!({"token":token,"path":path})
            )
            .is_err());
    }
    let local = Path::new(&plan.record.source).join("ref/a.md");
    let old = fs::read(&local).unwrap();
    fs::write(&local, "changed after preview").unwrap();
    assert!(m
        .admin_command(
            "plan_compare",
            serde_json::json!({"token":plan.token,"path":"SKILL.md"})
        )
        .unwrap_err()
        .contains("本地内容"));
    fs::write(&local, old).unwrap();
    fs::write(
        data.join("plans")
            .join(&plan.token)
            .join("payload/ref/a.md"),
        "changed payload",
    )
    .unwrap();
    assert!(m
        .admin_command(
            "plan_compare",
            serde_json::json!({"token":plan.token,"path":"SKILL.md"})
        )
        .unwrap_err()
        .contains("待安装内容"));
}
