use skill_manager::{
    global::{Discovery, Manager, Plan},
    registry::{self, Candidate},
};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tempfile::TempDir;
fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn fixture() -> (TempDir, PathBuf, PathBuf, Manager) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let home = root.join("home");
    let data = root.join("data");
    fs::create_dir_all(&home).unwrap();
    let manager = Manager::new(&home, &data).unwrap();
    (t, home, data, manager)
}
fn skill(path: &Path, version: &str) {
    write(
        &path.join("SKILL.md"),
        &format!("---\nname: demo\ndescription: Test skill\n---\n{version}\n"),
    );
    write(&path.join("references/guide.md"), version);
}
fn local(t: &TempDir, m: &Manager) -> Plan {
    let path = t.path().join("demo");
    skill(&path, "version 1");
    let p = m.prepare_local(&path).unwrap();
    m.apply(&p.token, false).unwrap();
    p
}
fn remote(data: &Path, m: &Manager, version: &str, existing: Option<&str>) -> Plan {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let token = format!("{}", SEQ.fetch_add(1, Ordering::Relaxed));
    let dir = data.join("discoveries").join(&token);
    skill(&dir.join("tree/nested/demo"), version);
    let d = Discovery {
        token: token.clone(),
        repo: "example/repo".into(),
        reference: "main".into(),
        candidates: vec![Candidate {
            path: "nested/demo".into(),
            name: "demo".into(),
            description: "Test".into(),
        }],
    };
    write(
        &dir.join("discovery.json"),
        &serde_json::to_string(&d).unwrap(),
    );
    m.prepare_remote(&token, "nested/demo", existing).unwrap()
}
#[test]
fn import_then_link_and_disable_preserves_source() {
    let (t, home, _, m) = fixture();
    let p = local(&t, &m);
    m.toggle(&p.record.id, vec!["Claude".into(), "Codex".into()], true)
        .unwrap();
    assert_eq!(
        fs::read_link(home.join(".codex/skills/demo")).unwrap(),
        Path::new(&p.record.source)
    );
    m.toggle(&p.record.id, vec!["Codex".into()], false).unwrap();
    assert!(!home.join(".codex/skills/demo").exists());
    assert!(Path::new(&p.record.source).join("SKILL.md").exists());
    assert!(home.join(".claude/skills/demo/SKILL.md").exists());
}

#[test]
fn supported_clients_can_be_enabled_and_disabled_independently() {
    for (client, relative) in [
        ("DeepSeek Harness", ".dsh/skills"),
        ("ZCode", ".zcode/skills"),
        ("Kimi", ".kimi/skills"),
    ] {
        let (t, home, _, m) = fixture();
        let p = local(&t, &m);
        m.toggle(&p.record.id, vec![client.into()], true).unwrap();
        let destination = home.join(relative).join("demo");
        assert_eq!(
            fs::read_link(&destination).unwrap(),
            Path::new(&p.record.source)
        );
        assert_eq!(m.inventory().unwrap().skills[0].clients[client], "link");
        assert!(!home.join(".codex/skills/demo").exists());

        m.toggle(&p.record.id, vec![client.into()], false).unwrap();
        assert!(!destination.exists());
        assert!(Path::new(&p.record.source).join("SKILL.md").exists());
    }
}

#[test]
fn recognizes_relative_existing_links_without_copying() {
    use std::os::unix::fs::symlink;
    let (_t, home, _, m) = fixture();
    let source = home.join(".agents/skills/demo");
    skill(&source, "local");
    fs::create_dir_all(home.join(".codex/skills")).unwrap();
    symlink("../../.agents/skills/demo", home.join(".codex/skills/demo")).unwrap();
    let inv = m.inventory().unwrap();
    assert_eq!(inv.skills.len(), 1);
    assert_eq!(inv.skills[0].clients["Codex"], "link");
    m.toggle(&inv.skills[0].id, vec!["Codex".into()], false)
        .unwrap();
    assert!(source.exists());
}
#[test]
fn same_names_different_sources_are_not_merged() {
    let (_t, home, _, m) = fixture();
    skill(&home.join(".claude/skills/demo"), "a");
    skill(&home.join(".codex/skills/demo"), "b");
    let inv = m.inventory().unwrap();
    assert_eq!(inv.skills.len(), 2);
    assert_ne!(inv.skills[0].id, inv.skills[1].id);
}
#[test]
fn batch_conflict_is_preflighted_before_any_link_is_created() {
    let (t, home, _, m) = fixture();
    let p = local(&t, &m);
    skill(&home.join(".codex/skills/demo"), "foreign");
    assert!(m
        .toggle(&p.record.id, vec!["Claude".into(), "Codex".into()], true)
        .unwrap_err()
        .contains("不会覆盖"));
    assert!(!home.join(".claude/skills/demo").exists());
    assert!(fs::read_to_string(home.join(".codex/skills/demo/SKILL.md"))
        .unwrap()
        .contains("foreign"));
}
#[test]
fn foreign_symlink_is_never_removed() {
    use std::os::unix::fs::symlink;
    let (t, home, _, m) = fixture();
    let p = local(&t, &m);
    let other = t.path().join("other");
    skill(&other, "foreign");
    fs::create_dir_all(home.join(".codex/skills")).unwrap();
    symlink(&other, home.join(".codex/skills/demo")).unwrap();
    assert!(m.toggle(&p.record.id, vec!["Codex".into()], false).is_err());
    assert_eq!(
        fs::read_link(home.join(".codex/skills/demo")).unwrap(),
        other
    );
}
#[test]
fn upgrade_updates_linked_clients_and_keeps_old_revision() {
    let (_t, home, data, m) = fixture();
    let first = remote(&data, &m, "one", None);
    m.apply(&first.token, false).unwrap();
    m.toggle(&first.record.id, vec!["Codex".into()], true)
        .unwrap();
    let next = remote(&data, &m, "two", Some(&first.record.id));
    assert!(!next.local_modified);
    assert_eq!(next.record.origin.as_ref().unwrap().path, "nested/demo");
    m.apply(&next.token, false).unwrap();
    assert!(fs::read_to_string(home.join(".codex/skills/demo/SKILL.md"))
        .unwrap()
        .contains("two"));
    let backups = m.backups(&first.record.id).unwrap();
    assert_eq!(backups.len(), 1);
    let restore = m
        .prepare_restore(&first.record.id, &backups[0].token)
        .unwrap();
    assert_eq!(restore.kind, "restore");
    m.apply(&restore.token, false).unwrap();
    assert!(fs::read_to_string(home.join(".codex/skills/demo/SKILL.md"))
        .unwrap()
        .contains("one"));
}
#[test]
fn modified_local_files_require_explicit_backup_approval() {
    let (_t, _, data, m) = fixture();
    let first = remote(&data, &m, "one", None);
    m.apply(&first.token, false).unwrap();
    write(
        &Path::new(&first.record.source).join("SKILL.md"),
        "local edits",
    );
    let next = remote(&data, &m, "two", Some(&first.record.id));
    assert!(next.local_modified);
    assert!(m.apply(&next.token, false).is_err());
    assert_eq!(
        fs::read_to_string(Path::new(&first.record.source).join("SKILL.md")).unwrap(),
        "local edits"
    );
    m.apply(&next.token, true).unwrap();
    let b = m.backups(&first.record.id).unwrap();
    assert_eq!(
        fs::read_to_string(
            data.join("backups")
                .join(&b[0].token)
                .join("content/SKILL.md")
        )
        .unwrap(),
        "local edits"
    );
}
#[test]
fn external_edit_after_preview_invalidates_approval() {
    let (_t, _, data, m) = fixture();
    let first = remote(&data, &m, "one", None);
    m.apply(&first.token, false).unwrap();
    let next = remote(&data, &m, "two", Some(&first.record.id));
    write(
        &Path::new(&first.record.source).join("SKILL.md"),
        "concurrent edit",
    );
    assert!(m
        .apply(&next.token, true)
        .unwrap_err()
        .contains("预览后本地内容"));
}
#[test]
fn metadata_changes_after_preview_are_not_overwritten() {
    let (_t, _, data, m) = fixture();
    let first = remote(&data, &m, "one", None);
    m.apply(&first.token, false).unwrap();
    let next = remote(&data, &m, "two", Some(&first.record.id));
    m.toggle(&first.record.id, vec!["Claude".into()], true)
        .unwrap();
    assert!(m
        .apply(&next.token, true)
        .unwrap_err()
        .contains("管理状态已变化"));
}
#[test]
fn upgrade_failure_at_each_move_restores_files_and_metadata() {
    for stage in [1, 2] {
        let (_t, _, data, m) = fixture();
        let first = remote(&data, &m, "one", None);
        m.apply(&first.token, false).unwrap();
        let next = remote(&data, &m, "two", Some(&first.record.id));
        assert!(m
            .apply_with_failure(&next.token, false, Some(stage))
            .unwrap_err()
            .contains("已回滚"));
        assert!(
            fs::read_to_string(Path::new(&first.record.source).join("SKILL.md"))
                .unwrap()
                .contains("one")
        );
        assert!(!m.inventory().unwrap().pending);
        assert!(!m.inventory().unwrap().skills[0].local_modified);
    }
}
#[test]
fn recovery_after_process_restart_rolls_back_file_and_db_commit() {
    let (_t, home, data, m) = fixture();
    let first = remote(&data, &m, "one", None);
    m.apply(&first.token, false).unwrap();
    let next = remote(&data, &m, "two", Some(&first.record.id));
    let j = serde_json::json!({"token":next.token,"plan":next,"before":next.before,"after":next.record,"links":[]});
    m.apply(&next.token, false).unwrap();
    write(&data.join("transaction.json"), &j.to_string());
    drop(m);
    let m = Manager::new(&home, &data).unwrap();
    assert!(m.inventory().unwrap().pending);
    assert!(m
        .toggle(&first.record.id, vec!["Claude".into()], true)
        .is_err());
    m.recover().unwrap();
    assert!(
        fs::read_to_string(Path::new(&first.record.source).join("SKILL.md"))
            .unwrap()
            .contains("one")
    );
    assert!(!m.inventory().unwrap().pending);
}
#[test]
fn restore_can_rebuild_a_missing_shared_source() {
    let (_t, _, data, m) = fixture();
    let first = remote(&data, &m, "one", None);
    m.apply(&first.token, false).unwrap();
    let next = remote(&data, &m, "two", Some(&first.record.id));
    m.apply(&next.token, false).unwrap();
    fs::remove_dir_all(&next.record.source).unwrap();
    let b = m.backups(&first.record.id).unwrap();
    let restore = m.prepare_restore(&first.record.id, &b[0].token).unwrap();
    m.apply(&restore.token, true).unwrap();
    assert!(Path::new(&first.record.source).join("SKILL.md").exists());
}
#[test]
fn interrupted_link_disable_restores_original_relative_target() {
    use std::os::unix::fs::symlink;
    let (t, home, data, m) = fixture();
    let p = local(&t, &m);
    let dest = home.join(".codex/skills/demo");
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let relative = "../../.cc-switch/skills/demo";
    symlink(relative, &dest).unwrap();
    let backup = data.join("backups/100");
    fs::create_dir_all(&backup).unwrap();
    let j = serde_json::json!({"token":"100","plan":null,"before":p.record,"after":p.record,
        "links":[{"client":"Codex","path":dest,"before":relative,"enable":false}]});
    write(&data.join("transaction.json"), &j.to_string());
    fs::rename(&dest, backup.join("link-0")).unwrap();
    drop(m);
    let m = Manager::new(&home, &data).unwrap();
    m.recover().unwrap();
    assert_eq!(fs::read_link(dest).unwrap(), Path::new(relative));
    assert!(!m.inventory().unwrap().pending);
}
#[test]
fn link_recovery_keeps_journal_when_external_process_removes_original() {
    let (t, home, data, m) = fixture();
    let p = local(&t, &m);
    let j = serde_json::json!({"token":"100","plan":null,"before":p.record,"after":p.record,
        "links":[{"client":"Codex","path":home.join(".codex/skills/demo"),"before":p.record.source,"enable":false}]});
    write(&data.join("transaction.json"), &j.to_string());
    assert!(m.recover().is_err());
    assert!(m.inventory().unwrap().pending);
    assert!(Path::new(&p.record.source).join("SKILL.md").exists());
}
#[test]
fn internal_symlinks_are_preserved_but_escapes_rejected() {
    use std::os::unix::fs::symlink;
    let (t, _, _, m) = fixture();
    let source = t.path().join("demo");
    skill(&source, "safe");
    symlink("references/guide.md", source.join("shortcut.md")).unwrap();
    let p = m.prepare_local(&source).unwrap();
    m.apply(&p.token, false).unwrap();
    assert_eq!(
        fs::read_link(Path::new(&p.record.source).join("shortcut.md")).unwrap(),
        Path::new("references/guide.md")
    );
    write(&t.path().join("private.md"), "outside");
    symlink("../private.md", source.join("escape.md")).unwrap();
    assert!(m.prepare_local(&source).unwrap_err().contains("越界软链接"));
}
#[test]
fn aliased_client_roots_share_one_physical_link() {
    use std::os::unix::fs::symlink;
    let (t, home, _, m) = fixture();
    let p = local(&t, &m);
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    fs::create_dir_all(home.join(".codex")).unwrap();
    symlink("../.claude/skills", home.join(".codex/skills")).unwrap();
    m.toggle(&p.record.id, vec!["Claude".into(), "Codex".into()], true)
        .unwrap();
    m.toggle(&p.record.id, vec!["Claude".into(), "Codex".into()], false)
        .unwrap();
    assert!(Path::new(&p.record.source).exists());
}
#[test]
fn root_source_directory_cannot_be_disabled() {
    let (_t, home, _, m) = fixture();
    skill(&home.join(".codex/skills/demo"), "source");
    let s = m.inventory().unwrap().skills.remove(0);
    assert_eq!(s.clients["Codex"], "source");
    assert!(m.toggle(&s.id, vec!["Codex".into()], false).is_err());
    assert!(home.join(".codex/skills/demo/SKILL.md").exists());
}
#[test]
fn external_link_removal_is_reported_as_drift() {
    let (t, home, _, m) = fixture();
    let p = local(&t, &m);
    m.toggle(&p.record.id, vec!["Codex".into()], true).unwrap();
    fs::remove_file(home.join(".codex/skills/demo")).unwrap();
    let s = m.inventory().unwrap().skills.remove(0);
    assert_eq!(s.drift, vec!["Codex"]);
}
#[test]
fn duplicate_install_and_tampered_preview_are_rejected() {
    let (t, _, data, m) = fixture();
    let first = local(&t, &m);
    assert!(m
        .prepare_local(&t.path().join("demo"))
        .unwrap_err()
        .contains("共享库已存在"));
    let next = remote(&data, &m, "two", Some(&first.record.id));
    write(
        &data
            .join("plans")
            .join(&next.token)
            .join("payload/SKILL.md"),
        "tampered",
    );
    assert!(m
        .apply(&next.token, true)
        .unwrap_err()
        .contains("待安装内容已变化"));
}
#[test]
fn registry_search_filters_invalid_sources_and_preserves_skill_id() {
    let json=br#"{"count":2,"skills":[{"source":"owner/repo","name":"My skill","skillId":"my-skill","installs":123},{"source":"owner/repo/escape","name":"bad","skillId":"bad"}]}"#;
    let result = registry::parse_search(json, "query").unwrap();
    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].skill_id, "my-skill");
    assert_eq!(result.skills[0].installs, 123);
}
fn zip(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        archive
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(data.as_bytes()).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
#[test]
fn archive_paths_cannot_escape_or_collide_by_case() {
    for entries in [
        vec![("repo/../outside", "bad")],
        vec![("repo/Foo", "a"), ("repo/foo", "b")],
        vec![("repo/safe", "a"), ("other/root", "b")],
    ] {
        let t = tempfile::tempdir().unwrap();
        assert!(registry::extract_archive(&zip(&entries), t.path()).is_err());
    }
}
#[test]
fn discovery_keeps_exact_nested_paths_and_repository_root_skills() {
    let t = tempfile::tempdir().unwrap();
    registry::extract_archive(
        &zip(&[("repo/nested/demo/SKILL.md", "---\nname: demo\n---\nbody")]),
        t.path(),
    )
    .unwrap();
    let c = registry::candidates(t.path()).unwrap();
    assert_eq!(c[0].path, "nested/demo");
    let root = tempfile::tempdir().unwrap();
    skill(root.path(), "root");
    assert_eq!(registry::candidates(root.path()).unwrap()[0].path, ".");
}

fn cached_readonly_skill(data: &Path) {
    skill(
        &data.join("discoveries/123456/tree/skills/demo"),
        "remote details",
    );
    let d = Discovery {
        token: "123456".into(),
        repo: "example/repo".into(),
        reference: "main".into(),
        candidates: vec![Candidate {
            path: "skills/demo".into(),
            name: "demo".into(),
            description: "Remote description".into(),
        }],
    };
    write(
        &data.join("discoveries/123456/discovery.json"),
        &serde_json::to_string(&d).unwrap(),
    );
}

#[test]
fn remote_details_are_readable_with_an_existing_skill_and_do_not_prepare_install() {
    let (_t, home, data, m) = fixture();
    let source = home.join(".cc-switch/skills/demo");
    skill(&source, "keep local");
    cached_readonly_skill(&data);
    let details = m.discovery_details("123456", "skills/demo").unwrap();
    assert_eq!(details.path, "skills/demo");
    assert_eq!(details.repo, "example/repo");
    assert!(details.content.ends_with("remote details\n"));
    assert_eq!(details.files, vec!["SKILL.md", "references/guide.md"]);
    assert!(fs::read_to_string(source.join("SKILL.md"))
        .unwrap()
        .contains("keep local"));
    assert!(!data.join("plans").exists());
    assert!(!home.join(".codex").exists());
    assert!(!m.inventory().unwrap().skills[0].managed);
}

#[test]
fn remote_details_reject_unknown_candidates_and_traversal() {
    let (_t, home, data, m) = fixture();
    cached_readonly_skill(&data);
    skill(&data.join("discoveries/123456/tree/other"), "unlisted");
    assert!(m.discovery_details("123456", "other").is_err());
    assert!(m.discovery_details("123456", "../private").is_err());
    assert!(m.discovery_details("../123456", "skills/demo").is_err());
    assert!(!home.join(".cc-switch").exists());
}

#[test]
fn remote_details_reject_a_candidate_redirected_outside_the_download() {
    use std::os::unix::fs::symlink;
    let (t, _home, data, m) = fixture();
    cached_readonly_skill(&data);
    let candidate = data.join("discoveries/123456/tree/skills/demo");
    fs::rename(&candidate, t.path().join("private")).unwrap();
    symlink(t.path().join("private"), &candidate).unwrap();
    assert!(m
        .discovery_details("123456", "skills/demo")
        .unwrap_err()
        .contains("超出"));
}

#[test]
fn remote_details_never_silently_truncate_the_skill_content() {
    let (_t, _home, data, m) = fixture();
    cached_readonly_skill(&data);
    let path = data.join("discoveries/123456/tree/skills/demo/SKILL.md");
    let text = format!("{}\nEND OF SKILL", "a".repeat(300 * 1024));
    fs::write(&path, &text).unwrap();
    assert_eq!(
        m.discovery_details("123456", "skills/demo")
            .unwrap()
            .content,
        text
    );
    fs::write(&path, vec![b'a'; 2 * 1024 * 1024 + 1]).unwrap();
    assert!(m.discovery_details("123456", "skills/demo").is_err());
}

#[test]
fn initial_download_survives_changing_upgrade_repository_and_restore() {
    use skill_manager::global::DownloadSource;
    let (_t, home, data, m) = fixture();
    let first = remote(&data, &m, "v1", None);
    m.apply(&first.token, false).unwrap();
    let initial = first.record.initial_source();
    assert!(matches!(initial, DownloadSource::Remote { .. }));
    let token = "987654321";
    let dir = data.join("discoveries").join(token);
    skill(&dir.join("tree/nested/demo"), "v2");
    let d = Discovery {
        token: token.into(),
        repo: "another/repository".into(),
        reference: "feature/new".into(),
        candidates: vec![Candidate {
            path: "nested/demo".into(),
            name: "demo".into(),
            description: "Test".into(),
        }],
    };
    write(
        &dir.join("discovery.json"),
        &serde_json::to_string(&d).unwrap(),
    );
    let upgrade = m
        .prepare_remote(token, "nested/demo", Some(&first.record.id))
        .unwrap();
    assert_ne!(upgrade.record.origin, first.record.origin);
    assert_eq!(upgrade.record.initial_source(), initial);
    m.apply(&upgrade.token, false).unwrap();
    drop(m);
    let m = Manager::new(&home, &data).unwrap();
    assert_eq!(m.inventory().unwrap().skills[0].download_source, initial);
    let restore = m.prepare_restore(&first.record.id, &upgrade.token).unwrap();
    m.apply(&restore.token, false).unwrap();
    assert_eq!(m.inventory().unwrap().skills[0].download_source, initial);
}

#[test]
fn local_import_source_is_not_replaced_by_later_repository_association() {
    use skill_manager::global::DownloadSource;
    let (t, _, data, m) = fixture();
    let first = local(&t, &m);
    let initial = first.record.initial_source();
    assert_eq!(
        initial,
        DownloadSource::Local {
            path: t
                .path()
                .join("demo")
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into()
        }
    );
    let upgrade = remote(&data, &m, "v2", Some(&first.record.id));
    m.apply(&upgrade.token, true).unwrap();
    assert_eq!(m.inventory().unwrap().skills[0].download_source, initial);
}

#[test]
fn legacy_records_still_load_with_or_without_origin() {
    use skill_manager::global::{DownloadSource, Record};
    let mut value = serde_json::json!({"id":"old", "name":"demo", "source":"/tmp/demo", "origin":null, "baseline":"hash", "expected":[]});
    let record: Record = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(record.initial_source(), DownloadSource::Unknown);
    value["origin"] = serde_json::json!({"repo":"example/repo","reference":"main","path":"."});
    let record: Record = serde_json::from_value(value).unwrap();
    assert!(matches!(
        record.initial_source(),
        DownloadSource::Remote { .. }
    ));
}

#[test]
fn source_page_url_encodes_paths_and_rejects_non_repository_input() {
    use skill_manager::registry::Origin;
    let mut origin = Origin {
        repo: "example/repo".into(),
        reference: "feature/new".into(),
        path: "skills/my skill".into(),
    };
    assert_eq!(
        registry::source_page_url(&origin).unwrap(),
        "https://github.com/example/repo/tree/feature%2Fnew/skills/my%20skill"
    );
    origin.path = ".".into();
    assert_eq!(
        registry::source_page_url(&origin).unwrap(),
        "https://github.com/example/repo/tree/feature%2Fnew"
    );
    origin.path = "../escape".into();
    assert!(registry::source_page_url(&origin).is_err());
    origin.path = ".".into();
    origin.repo = "https://evil.example/repo".into();
    assert!(registry::source_page_url(&origin).is_err());
}

#[test]
fn translation_updates_global_source_and_link_without_losing_backup() {
    let (t, home, data, m) = fixture();
    let p = local(&t, &m);
    let id = &p.record.id;
    m.toggle(id, vec!["Codex".into()], true).unwrap();
    let original = m.details(id).unwrap();
    let translated = original.replace("version 1", "版本一");
    let backup = m
        .replace_translation(
            id,
            &original,
            &translated,
            &data.join("translation-backups"),
        )
        .unwrap();
    assert_eq!(m.details(id).unwrap(), translated);
    assert_eq!(
        fs::read_to_string(home.join(".codex/skills/demo/SKILL.md")).unwrap(),
        translated
    );
    assert_eq!(fs::read_to_string(backup.backup_path).unwrap(), original);
    assert!(
        m.inventory()
            .unwrap()
            .skills
            .iter()
            .find(|s| &s.id == id)
            .unwrap()
            .local_modified
    );
    m.replace_translation(
        id,
        &translated,
        &original,
        &data.join("translation-backups"),
    )
    .unwrap();
    assert_eq!(m.details(id).unwrap(), original);
}

#[test]
fn retired_clients_are_not_scanned_or_modified() {
    let (t, home, _, m) = fixture();
    let p = local(&t, &m);
    for (client, relative) in [
        ("Gemini", ".gemini/skills"),
        ("GrokBuild", ".grok/skills"),
        ("Hermes", ".hermes/skills"),
        ("Pi", ".pi/agent/skills"),
    ] {
        let source = home.join(relative).join("old-skill");
        skill(&source, "preserve");
        assert!(m.toggle(&p.record.id, vec![client.into()], true).is_err());
        assert!(source.join("SKILL.md").exists());
        assert!(!m.inventory().unwrap().skills[0]
            .clients
            .contains_key(client));
    }
    assert_eq!(m.inventory().unwrap().skills.len(), 1);
}
