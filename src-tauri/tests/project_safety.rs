use skill_manager::{
    engine::{tree_digest, Change, Repo},
    store::Store,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;
const SKILL: &str = ".cursor/skills/activity-dev";
const RULE: &str = ".cursor/rules/performance.mdc";
fn git(p: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(p)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap()
}
fn ok(p: &Path, args: &[&str]) -> Vec<u8> {
    let r = git(p, args);
    assert!(
        r.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&r.stderr)
    );
    r.stdout
}
fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}
fn fixture() -> (TempDir, Repo) {
    let t = TempDir::new().unwrap();
    let p = t.path().canonicalize().unwrap();
    ok(&p, &["init", "-b", "main"]);
    ok(&p, &["config", "user.name", "Fixture"]);
    ok(&p, &["config", "user.email", "test@example.test"]);
    write(
        &p.join(SKILL).join("SKILL.md"),
        "---\nname: activity-dev\ndescription: Activity development\n---\n# Body\n",
    );
    write(&p.join(SKILL).join("references/guide.md"), "Reference\n");
    write(&p.join(RULE), "---\ndescription: Performance\n---\nRule\n");
    ok(&p, &["add", "."]);
    ok(&p, &["commit", "-m", "initial"]);
    (t, Repo::open(&p).unwrap())
}
fn change(path: &str, enable: bool) -> Change {
    Change {
        path: path.into(),
        enable,
    }
}
fn hook_path(repo: &Repo) -> PathBuf {
    PathBuf::from(
        String::from_utf8(ok(
            &repo.root,
            &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
        ))
        .unwrap()
        .trim(),
    )
    .join("pre-commit")
}
#[test]
fn scan_is_read_only_and_lists_skills_rules() {
    let (_t, r) = fixture();
    let s = r.scan().unwrap();
    assert_eq!(s.resources.len(), 2);
    assert!(!r.vault.exists());
    assert_eq!(s.hook, "missing");
    assert!(s.resources.iter().any(|r| r.kind == "rule"));
}

#[test]
fn scan_lists_supported_project_skills() {
    for (provider, path) in [
        ("DeepSeek Harness", ".dsh/skills/demo"),
        ("ZCode", ".zcode/skills/demo"),
        ("Kimi", ".kimi/skills/demo"),
    ] {
        let (_t, r) = fixture();

        write(
            &r.root.join(path).join("SKILL.md"),
            "---\nname: dsh-project-skill\ndescription: DSH project skill\n---\n# Body\n",
        );
        let snapshot = r.scan().unwrap();
        let resource = snapshot
            .resources
            .iter()
            .find(|resource| resource.path == path)
            .unwrap();
        assert_eq!(resource.kind, "skill");
        assert_eq!(resource.provider, provider);
        assert!(resource.enabled);
    }
}

#[test]
fn update_dates_include_skill_files_and_survive_parking() {
    use std::time::{Duration, UNIX_EPOCH};
    let (_t, r) = fixture();
    let set_modified = |path: PathBuf, seconds| {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    };
    set_modified(r.root.join(SKILL).join("SKILL.md"), 1_700_000_000);
    set_modified(
        r.root.join(SKILL).join("references/guide.md"),
        1_700_000_100,
    );
    set_modified(r.root.join(RULE), 1_700_000_200);
    let check = || {
        let snapshot = r.scan().unwrap();
        for (path, expected) in [(SKILL, 1_700_000_100_000), (RULE, 1_700_000_200_000)] {
            let resource = snapshot
                .resources
                .iter()
                .find(|item| item.path == path)
                .unwrap();
            assert_eq!(resource.updated_at, Some(expected));
            assert_eq!(
                serde_json::to_value(resource).unwrap()["updatedAt"],
                expected
            );
        }
    };
    check();
    r.install_hook(false).unwrap();
    r.apply(vec![change(SKILL, false), change(RULE, false)])
        .unwrap();
    check();
    r.apply(vec![change(SKILL, true), change(RULE, true)])
        .unwrap();
    check();
}

#[test]
fn disable_requires_guard() {
    let (_t, r) = fixture();
    assert!(r.apply(vec![change(SKILL, false)]).is_err());
    assert!(r.root.join(SKILL).exists());
}
#[test]
fn roundtrip_preserves_dirty_worktree_staged_content_modes_and_blocks_commit() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, r) = fixture();
    let file = r.root.join(SKILL).join("SKILL.md");
    write(&file, "Staged version\n");
    ok(&r.root, &["add", SKILL]);
    write(&file, "Unstaged version\n");
    let script = r.root.join(SKILL).join("script.sh");
    write(&script, "#!/bin/sh\necho hello\n");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let before = tree_digest(&r.root.join(SKILL)).unwrap();
    let index = ok(&r.root, &["ls-files", "--stage", "-z"]);
    let status = ok(&r.root, &["status", "--porcelain=v1"]);
    r.install_hook(false).unwrap();
    r.apply(vec![change(SKILL, false), change(RULE, false)])
        .unwrap();
    assert!(!r.root.join(SKILL).exists());
    assert!(!r.root.join(RULE).exists());
    assert_eq!(
        r.scan()
            .unwrap()
            .resources
            .iter()
            .filter(|r| !r.enabled)
            .count(),
        2
    );
    let commit = git(&r.root, &["commit", "--allow-empty", "-m", "must block"]);
    assert!(!commit.status.success());
    assert!(String::from_utf8_lossy(&commit.stderr).contains("Skill Manager"));
    r.apply(vec![change(SKILL, true), change(RULE, true)])
        .unwrap();
    assert_eq!(before, tree_digest(&r.root.join(SKILL)).unwrap());
    assert_eq!(index, ok(&r.root, &["ls-files", "--stage", "-z"]));
    assert_eq!(status, ok(&r.root, &["status", "--porcelain=v1"]));
    ok(&r.root, &["commit", "-m", "restored"]);
}
#[test]
fn preflight_collision_does_not_partially_restore() {
    let (_t, r) = fixture();
    r.install_hook(false).unwrap();
    r.apply(vec![change(SKILL, false), change(RULE, false)])
        .unwrap();
    write(&r.root.join(RULE), "External content");
    assert!(r
        .apply(vec![change(SKILL, true), change(RULE, true)])
        .unwrap_err()
        .contains("已有内容"));
    assert!(!r.root.join(SKILL).exists());
    assert_eq!(
        fs::read_to_string(r.root.join(RULE)).unwrap(),
        "External content"
    );
}
#[test]
fn injected_mid_batch_failure_rolls_back_everything() {
    let (_t, r) = fixture();
    r.install_hook(false).unwrap();
    let before = ok(&r.root, &["status", "--porcelain"]);
    let e = r
        .apply_inner(vec![change(SKILL, false), change(RULE, false)], Some(1))
        .unwrap_err();
    assert!(e.contains("已完整回滚"));
    assert_eq!(before, ok(&r.root, &["status", "--porcelain"]));
    assert!(!r.vault.join("transaction.json").exists());
    ok(&r.root, &["commit", "--allow-empty", "-m", "rollback safe"]);
}
#[test]
fn staged_deletion_requires_index_repair_before_restore() {
    let (_t, r) = fixture();
    r.install_hook(false).unwrap();
    r.apply(vec![change(SKILL, false)]).unwrap();
    ok(&r.root, &["add", "-u"]);
    assert!(r
        .apply(vec![change(SKILL, true)])
        .unwrap_err()
        .contains("暂存区发生变化"));
    assert!(r
        .scan()
        .unwrap()
        .resources
        .iter()
        .any(|r| r.status == "drift"));
    ok(&r.root, &["reset", "HEAD", "--", SKILL]);
    r.apply(vec![change(SKILL, true)]).unwrap();
}
#[test]
fn branch_drift_is_preserved_without_overwrite() {
    let (_t, r) = fixture();
    r.install_hook(false).unwrap();
    r.apply(vec![change(SKILL, false)]).unwrap();
    ok(
        &r.root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "-m",
            "new head",
        ],
    );
    assert!(r
        .apply(vec![change(SKILL, true)])
        .unwrap_err()
        .contains("分支"));
    assert!(!r.root.join(SKILL).exists());
}
#[test]
fn corrupt_manifest_blocks_commits_and_is_not_reinitialized() {
    let (_t, r) = fixture();
    r.install_hook(false).unwrap();
    fs::write(r.vault.join("state.json"), "broken").unwrap();
    assert!(r.scan().is_err());
    assert!(r.install_hook(false).is_err());
    assert!(!git(&r.root, &["commit", "--allow-empty", "-m", "blocked"])
        .status
        .success());
}
#[test]
fn missing_manifest_with_backups_is_not_reinitialized() {
    let (_t, r) = fixture();
    r.install_hook(false).unwrap();
    r.apply(vec![change(SKILL, false)]).unwrap();
    fs::remove_file(r.vault.join("state.json")).unwrap();
    assert!(r.install_hook(false).is_err());
    assert!(r
        .vault
        .join("resources")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
}
#[test]
fn tampered_backup_is_kept_and_never_restored_over_current_files() {
    let (_t, r) = fixture();
    r.install_hook(false).unwrap();
    r.apply(vec![change(SKILL, false)]).unwrap();
    let backup = r
        .vault
        .join("resources")
        .read_dir()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    write(&backup.join("SKILL.md"), "Changed backup");
    assert!(r
        .apply(vec![change(SKILL, true)])
        .unwrap_err()
        .contains("暂存内容已变化"));
    assert_eq!(
        fs::read_to_string(backup.join("SKILL.md")).unwrap(),
        "Changed backup"
    );
}
#[test]
fn existing_hook_requires_explicit_chain_and_still_runs() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, r) = fixture();
    let hook = hook_path(&r);
    write(&hook, "#!/bin/sh\necho previous-hook >&2\nexit 42\n");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(r.install_hook(false).is_err());
    assert_eq!(
        fs::read_to_string(&hook).unwrap(),
        "#!/bin/sh\necho previous-hook >&2\nexit 42\n"
    );
    r.install_hook(true).unwrap();
    let out = git(&r.root, &["commit", "--allow-empty", "-m", "chain"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("previous-hook"));
}
#[test]
fn custom_hook_path_is_never_modified() {
    let (_t, r) = fixture();
    ok(&r.root, &["config", "core.hooksPath", ".custom-hooks"]);
    assert_eq!(r.hook_status(), "custom");
    assert!(r.install_hook(true).is_err());
    assert!(!r.root.join(".custom-hooks").exists());
}
#[test]
fn symlinks_are_reported_and_not_moved() {
    use std::os::unix::fs::symlink;
    let (_t, r) = fixture();
    symlink("references/guide.md", r.root.join(SKILL).join("linked.md")).unwrap();
    r.install_hook(false).unwrap();
    assert!(r
        .scan()
        .unwrap()
        .resources
        .iter()
        .any(|r| r.status == "unsupported"));
    assert!(r.apply(vec![change(SKILL, false)]).is_err());
    assert!(r.root.join(SKILL).exists());
}
#[test]
fn path_traversal_is_rejected() {
    let (_t, r) = fixture();
    r.install_hook(false).unwrap();
    for p in [
        "../outside",
        ".cursor/skills/../../AGENTS.md",
        ".cursor/skills/activity-dev/sub",
        "/etc/passwd",
    ] {
        assert!(r.apply(vec![change(p, false)]).is_err());
    }
}
#[test]
fn worktrees_have_separate_vaults_and_shared_guard() {
    let (t, r) = fixture();
    let work = t.path().join("work tree");
    ok(
        &r.root,
        &["worktree", "add", "-b", "task", work.to_str().unwrap()],
    );
    let w = Repo::open(&work).unwrap();
    assert_ne!(r.vault, w.vault);
    r.install_hook(false).unwrap();
    w.install_hook(false).unwrap();
    w.apply(vec![change(SKILL, false)]).unwrap();
    assert!(r.root.join(SKILL).exists());
    assert!(!w.root.join(SKILL).exists());
    ok(&r.root, &["commit", "--allow-empty", "-m", "main safe"]);
    assert!(!git(
        &w.root,
        &["commit", "--allow-empty", "-m", "worktree block"]
    )
    .status
    .success());
    w.apply(vec![change(SKILL, true)]).unwrap();
}
#[test]
fn submodule_git_file_uses_private_git_directory() {
    let (_source, s) = fixture();
    let (_parent, p) = fixture();
    let module = p.root.join("child");
    ok(
        &p.root,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            s.root.to_str().unwrap(),
            "child",
        ],
    );
    let r = Repo::open(&module).unwrap();
    assert!(module.join(".git").is_file());
    assert!(r.vault.starts_with(p.root.join(".git/modules")));
    r.install_hook(false).unwrap();
    r.apply(vec![change(SKILL, false)]).unwrap();
    r.apply(vec![change(SKILL, true)]).unwrap();
    assert!(module.join(SKILL).exists());
}
#[test]
fn nested_repository_is_not_moved_as_a_skill() {
    let (_t, r) = fixture();
    let nested = r.root.join(SKILL);
    ok(&nested, &["init"]);
    r.install_hook(false).unwrap();
    assert!(r
        .apply(vec![change(SKILL, false)])
        .unwrap_err()
        .contains("嵌套仓库"));
    assert!(nested.exists());
}
#[test]
fn restart_recovers_journal_written_before_and_after_move() {
    for move_done in [false, true] {
        let (_t, r) = fixture();
        r.install_hook(false).unwrap();
        let before: serde_json::Value =
            serde_json::from_slice(&fs::read(r.vault.join("state.json")).unwrap()).unwrap();
        let digest = tree_digest(&r.root.join(SKILL)).unwrap();
        // Capture the actual backup location via a complete operation, then restore.
        r.apply(vec![change(SKILL, false)]).unwrap();
        let parked = r
            .vault
            .join("resources")
            .read_dir()
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        r.apply(vec![change(SKILL, true)]).unwrap();
        let j = serde_json::json!({"before":before,"moves":[{"path":SKILL,"enable":false,"digest":digest}]});
        fs::remove_file(r.vault.join("ready.hash")).unwrap();
        fs::write(
            r.vault.join("transaction.json"),
            serde_json::to_vec(&j).unwrap(),
        )
        .unwrap();
        if move_done {
            fs::rename(r.root.join(SKILL), parked).unwrap();
        }
        let reopened = Repo::open(&r.root).unwrap();
        assert!(reopened.scan().unwrap().pending);
        assert!(
            !git(&r.root, &["commit", "--allow-empty", "-m", "crash block"])
                .status
                .success()
        );
        reopened.recover().unwrap();
        assert!(r.root.join(SKILL).exists());
        assert!(!r.vault.join("transaction.json").exists());
        ok(&r.root, &["commit", "--allow-empty", "-m", "recovered"]);
    }
}
#[test]
fn profiles_and_groups_persist_in_own_database() {
    let (t, r) = fixture();
    let file = t.path().join("manager.sqlite");
    let s = Store::open(&file).unwrap();
    let root = s.add(r.root.to_str().unwrap()).unwrap();
    s.save(&root, "活动开发", "profile", vec![SKILL.into()])
        .unwrap();
    s.save(&root, "项目规则", "group", vec![RULE.into()])
        .unwrap();
    drop(s);
    let s = Store::open(&file).unwrap();
    assert_eq!(s.projects().unwrap().len(), 1);
    assert_eq!(s.collections(&root).unwrap().len(), 1);
    let presets = s.presets(&root).unwrap();
    assert_eq!(presets[0].name, "活动开发");
    assert_eq!(presets[0].default_state, "on");
    assert_eq!(presets[0].resources.get(SKILL), Some(&false));
    assert!(!r.vault.exists());
}

#[test]
fn shared_codex_link_does_not_duplicate_or_break_cursor_scan() {
    use std::os::unix::fs::symlink;
    let (_t, r) = fixture();
    fs::create_dir_all(r.root.join(".agents")).unwrap();
    symlink("../.cursor/skills", r.root.join(".agents/skills")).unwrap();
    let s = r.scan().unwrap();
    assert_eq!(s.resources.len(), 2);
    assert!(s.warnings.iter().any(|w| w.contains("链接入口")));
    r.install_hook(false).unwrap();
    r.apply(vec![change(SKILL, false)]).unwrap();
    assert!(!r.root.join(".agents/skills/activity-dev/SKILL.md").exists());
    r.apply(vec![change(SKILL, true)]).unwrap();
    assert!(r.root.join(".agents/skills/activity-dev/SKILL.md").exists());
}

#[test]
fn folded_frontmatter_description_is_readable() {
    let text="---\nname: example\ndescription: >-\n  First line\n  second line.\nlicense: MIT\n---\n# Body";
    assert_eq!(
        skill_manager::engine::frontmatter(text, "description"),
        Some("First line second line.".into())
    );
}

#[test]
fn orphaned_backup_cannot_be_certified_as_safe() {
    let (_t, r) = fixture();
    r.install_hook(false).unwrap();
    write(
        &r.vault.join("resources/orphan/SKILL.md"),
        "Recovered content",
    );
    assert!(r
        .recover()
        .unwrap_err()
        .contains("清单为空但暂存目录仍有文件"));
    assert!(
        !git(&r.root, &["commit", "--allow-empty", "-m", "must block"])
            .status
            .success()
    );
}

#[test]
fn translated_project_content_preserves_index_and_cannot_edit_parked_resources() {
    let (t, r) = fixture();
    let original = r.content(SKILL).unwrap();
    let translated = original.replace("# Body", "# 正文");
    let index = ok(&r.root, &["ls-files", "--stage", "-z"]);
    let backup_dir = t.path().join("translation-backups");
    let backup = r
        .replace_translation(SKILL, &original, &translated, &backup_dir)
        .unwrap();
    assert_eq!(fs::read_to_string(backup.backup_path).unwrap(), original);
    assert_eq!(r.content(SKILL).unwrap(), translated);
    assert!(
        !r.vault.exists(),
        "translation must not initialize or corrupt the parking vault"
    );
    assert_eq!(index, ok(&r.root, &["ls-files", "--stage", "-z"]));
    r.install_hook(false).unwrap();
    r.apply(vec![change(SKILL, false)]).unwrap();
    assert!(r
        .replace_translation(SKILL, &translated, &original, &backup_dir)
        .is_err());
    r.apply(vec![change(SKILL, true)]).unwrap();
    r.replace_translation(SKILL, &translated, &original, &backup_dir)
        .unwrap();
    assert_eq!(r.content(SKILL).unwrap(), original);
    let rule = r.content(RULE).unwrap();
    r.replace_translation(RULE, &rule, &rule.replace("Rule", "规则"), &backup_dir)
        .unwrap();
    assert!(r.content(RULE).unwrap().ends_with("规则\n"));
}

#[test]
fn project_groups_move_atomically_preserve_profiles_and_disabled_files() {
    let (_t, repo) = fixture();
    let data = TempDir::new().unwrap();
    let db = data.path().join("manager.sqlite");
    let store = Store::open(&db).unwrap();
    let root = store.add(repo.root.to_str().unwrap()).unwrap();
    store
        .save(&root, "A", "group", vec![SKILL.into(), RULE.into()])
        .unwrap();
    store.save(&root, "B", "group", vec![SKILL.into()]).unwrap();
    store
        .save(&root, "Scenario", "profile", vec![SKILL.into()])
        .unwrap();
    let groups = store.collections(&root).unwrap();
    let a = groups.iter().find(|c| c.name == "A").unwrap().id;
    let b = groups.iter().find(|c| c.name == "B").unwrap().id;
    repo.install_hook(false).unwrap();
    repo.apply(vec![change(SKILL, false)]).unwrap();
    store
        .move_group(&root, vec![SKILL.into(), RULE.into()], Some(b))
        .unwrap();
    store.update_group(&root, b, "Moved", "emerald").unwrap();
    store.reorder_groups(&root, vec![b, a]).unwrap();
    let before = serde_json::to_value(store.collections(&root).unwrap()).unwrap();
    assert!(store
        .move_group(
            &root,
            vec![RULE.into(), ".cursor/skills/missing".into()],
            Some(a)
        )
        .is_err());
    assert!(store
        .move_group(&root, vec![RULE.into()], Some(999))
        .is_err());
    assert!(store.reorder_groups(&root, vec![a, a]).is_err());
    assert_eq!(
        before,
        serde_json::to_value(store.collections(&root).unwrap()).unwrap()
    );
    drop(store);
    let store = Store::open(&db).unwrap();
    let rows = store.collections(&root).unwrap();
    assert_eq!(rows[0].id, b);
    assert_eq!(rows[0].color, "emerald");
    assert_eq!(rows[0].paths, vec![SKILL, RULE]);
    assert!(rows[1].paths.is_empty());
    assert_eq!(
        store.presets(&root).unwrap()[0].resources.get(SKILL),
        Some(&false)
    );
    assert!(!repo.root.join(SKILL).exists());
    store.move_group(&root, vec![SKILL.into()], None).unwrap();
    assert_eq!(store.collections(&root).unwrap()[0].paths, vec![RULE]);
    repo.apply(vec![change(SKILL, true)]).unwrap();
    assert!(repo.root.join(SKILL).join("SKILL.md").exists());
}

#[test]
fn legacy_collection_database_migrates_without_membership_loss() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("legacy.sqlite");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE projects(root TEXT PRIMARY KEY,name TEXT NOT NULL);
        CREATE TABLE collections(id INTEGER PRIMARY KEY,root TEXT NOT NULL,name TEXT NOT NULL,kind TEXT NOT NULL,paths TEXT NOT NULL,UNIQUE(root,name,kind));
        INSERT INTO collections VALUES(7,'/fixture','Old group','group','[\".cursor/skills/a\"]');").unwrap();
    drop(conn);
    let store = Store::open(&path).unwrap();
    let groups = store.collections("/fixture").unwrap();
    assert_eq!(groups[0].id, 7);
    assert_eq!(groups[0].color, "slate");
    assert_eq!(groups[0].paths, vec![".cursor/skills/a"]);
}

#[test]
fn presets_persist_edits_and_are_isolated_by_scope_without_changing_resources() {
    use skill_manager::store::Preset;
    let (t, repo) = fixture();
    let db = t.path().join("presets.sqlite");
    let store = Store::open(&db).unwrap();
    let root = store.add(repo.root.to_str().unwrap()).unwrap();
    let mut p = Preset {
        id: "one".into(),
        name: "方案一".into(),
        default_state: "keep".into(),
        groups: [("1".into(), false)].into(),
        resources: [(RULE.into(), true)].into(),
        clients: vec![],
    };
    store.save_preset(&root, p.clone()).unwrap();
    p.clients = vec![
        "Codex".into(),
        "DeepSeek Harness".into(),
        "ZCode".into(),
        "Kimi".into(),
    ];
    store.save_preset("global", p.clone()).unwrap();
    p.name = "方案二".into();
    p.groups.insert("1".into(), true);
    store.save_preset(&root, p).unwrap();
    drop(store);
    let store = Store::open(&db).unwrap();
    assert_eq!(store.presets(&root).unwrap()[0].name, "方案二");
    assert_eq!(store.presets("global").unwrap()[0].name, "方案一");
    store.delete_preset(&root, "one").unwrap();
    assert!(store.presets(&root).unwrap().is_empty());
    assert_eq!(store.presets("global").unwrap().len(), 1);
    assert!(repo.root.join(RULE).exists());
    assert!(repo.root.join(SKILL).exists());
    assert!(!repo.vault.exists());
}

#[test]
fn legacy_presets_migrate_once_and_do_not_reappear_after_deletion() {
    let (t, repo) = fixture();
    let db = t.path().join("legacy-presets.sqlite");
    let store = Store::open(&db).unwrap();
    let root = store.add(repo.root.to_str().unwrap()).unwrap();
    store
        .save(&root, "旧场景", "profile", vec![SKILL.into()])
        .unwrap();
    drop(store);
    let store = Store::open(&db).unwrap();
    let p = store.presets(&root).unwrap().remove(0);
    assert_eq!(p.default_state, "on");
    assert_eq!(p.resources.get(SKILL), Some(&false));
    store.delete_preset(&root, &p.id).unwrap();
    drop(store);
    assert!(Store::open(&db).unwrap().presets(&root).unwrap().is_empty());
}

#[test]
fn legacy_presets_drop_retired_targets_and_can_be_saved_again() {
    let t = TempDir::new().unwrap();
    let path = t.path().join("presets.sqlite");
    let store = Store::open(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let body = serde_json::json!({"id":"legacy","name":"Legacy","defaultState":"keep","groups":{},"resources":{},"clients":["Codex","Gemini","GrokBuild","Hermes","Pi"]});
    conn.execute(
        "INSERT INTO presets(scope,id,name,body) VALUES('global','legacy','Legacy',?1)",
        [body.to_string()],
    )
    .unwrap();
    let preset = store.presets("global").unwrap().remove(0);
    assert_eq!(preset.clients, vec!["Codex"]);
    store.save_preset("global", preset).unwrap();
}
