//! Read-only Codex rollout statistics. Never execute commands found in a log.
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    pub name: String,
    pub path: String,
    pub explicit: usize,
    pub inferred: usize,
    pub sessions: usize,
    pub last_used: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub rows: Vec<UsageRow>,
    pub files: usize,
    pub first_record: Option<String>,
    pub last_record: Option<String>,
    pub warnings: Vec<String>,
    pub log_root: String,
}
#[derive(Clone)]
struct Event {
    path: String,
    session: String,
    turn: String,
    time: String,
    explicit: bool,
}
#[derive(Clone, Default)]
struct Parsed {
    events: Vec<Event>,
    first: Option<String>,
    last: Option<String>,
    bad: usize,
}
type Cache = HashMap<PathBuf, (u64, SystemTime, Parsed)>;
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

fn collect(dir: &Path, files: &mut Vec<PathBuf>, warnings: &mut Vec<String>) {
    if !dir.exists() {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            warnings.push(format!("无法读取 {}：{e}", dir.display()));
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            warnings.push("部分日志目录项无法读取".into());
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect(&entry.path(), files, warnings);
        } else if kind.is_file() && entry.path().extension().is_some_and(|e| e == "jsonl") {
            files.push(entry.path());
        }
    }
}

fn skill_path(raw: &str, cwd: &str, home: &Path) -> Option<String> {
    if !raw.ends_with("/SKILL.md") || raw.contains(['$', '`', '\n', '*', '?']) {
        return None;
    }
    let path = if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else if !cwd.is_empty() {
        Path::new(cwd).join(raw)
    } else {
        return None;
    };
    // Lexically normalize even when an old skill no longer exists.
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            p => normalized.push(p.as_os_str()),
        }
    }
    Some(
        fs::canonicalize(&normalized)
            .unwrap_or(normalized)
            .to_string_lossy()
            .into_owned(),
    )
}

// Small shell lexer: only literal read commands are accepted. Expansions,
// redirections and compound commands are deliberately not inferred.
fn words(command: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        if c == '\\' && quote != Some('\'') {
            word.push(chars.next()?);
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            ';' | '|' | '&' | '>' | '<' | '`' | '$' | '\n' => return None,
            c if c.is_whitespace() => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    if !word.is_empty() {
        out.push(word);
    }
    Some(out)
}
fn read_paths(command: &str, cwd: &str, home: &Path) -> Vec<String> {
    let Some(w) = words(command) else {
        return vec![];
    };
    let Some(program) = w
        .first()
        .and_then(|s| Path::new(s).file_name())
        .and_then(|s| s.to_str())
    else {
        return vec![];
    };
    if !matches!(program, "cat" | "head" | "tail" | "sed") {
        return vec![];
    }
    if program == "sed" {
        // Only sed -n '<line range>p' FILE, never edits, script files or arbitrary scripts.
        if w.len() < 4
            || w[1] != "-n"
            || !w[2].ends_with('p')
            || !w[2][..w[2].len() - 1]
                .chars()
                .all(|c| c.is_ascii_digit() || c == ',')
        {
            return vec![];
        }
    }
    w.iter()
        .skip(if program == "sed" { 3 } else { 1 })
        .filter_map(|s| skill_path(s, cwd, home))
        .collect()
}

// Extract only literal exec_command object arguments from the JS orchestration
// wrapper; do not search arbitrary code strings for SKILL.md.
fn wrapped_commands(code: &str) -> Vec<Value> {
    let mut result = Vec::new();
    let mut rest = code;
    while let Some(pos) = rest.find("tools.exec_command(") {
        rest = &rest[pos + "tools.exec_command(".len()..];
        let trimmed = rest.trim_start();
        if !trimmed.starts_with('{') {
            continue;
        }
        let mut map = serde_json::Map::new();
        let mut s = &trimmed[1..];
        loop {
            s = s.trim_start();
            if s.starts_with('}') {
                result.push(Value::Object(map));
                break;
            }
            let key_end = s.find(':');
            let Some(key_end) = key_end else {
                break;
            };
            let key = s[..key_end].trim().trim_matches('"');
            if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                break;
            }
            s = s[key_end + 1..].trim_start();
            let mut stream = serde_json::Deserializer::from_str(s).into_iter::<Value>();
            let Some(Ok(value)) = stream.next() else {
                break;
            };
            let used = stream.byte_offset();
            map.insert(key.to_owned(), value);
            s = s[used..].trim_start();
            if s.starts_with(',') {
                s = &s[1..];
            } else if !s.starts_with('}') {
                break;
            }
        }
    }
    result
}

fn texts(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_owned();
    }
    if let Some(a) = value.as_array() {
        return a
            .iter()
            .filter_map(|v| v.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}
fn output_succeeded(output: &Value) -> bool {
    let parts: Vec<String> = match output.as_array() {
        Some(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str().map(str::to_owned))
            .collect(),
        None => vec![texts(output)],
    };
    let mut codes = Vec::new();
    for text in parts {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            let value = value.get("value").unwrap_or(&value);
            if let Some(code) = value.get("exit_code") {
                codes.push(code.as_i64());
            }
        } else {
            // Read the transport header only: skill bodies can quote error messages.
            let header = text.split("Output:").next().unwrap_or("");
            if header.starts_with("Script failed") {
                return false;
            }
            if let Some((_, code)) = header.split_once("Process exited with code ") {
                codes.push(code.split_whitespace().next().and_then(|s| s.parse().ok()));
            }
        }
    }
    !codes.is_empty() && codes.iter().all(|code| *code == Some(0))
}
fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    s.split_once(&format!("<{name}>"))?
        .1
        .split_once(&format!("</{name}>"))
        .map(|p| p.0.trim())
}

fn parse(path: &Path, home: &Path) -> std::io::Result<Parsed> {
    let mut parsed = Parsed::default();
    let mut session = path.to_string_lossy().into_owned();
    let mut cwd = String::new();
    let mut turn = String::from("session");
    let mut pending: HashMap<String, Vec<Event>> = HashMap::new();
    for line in BufReader::new(fs::File::open(path)?).lines() {
        let line = line?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            parsed.bad += 1;
            continue;
        };
        let time = v["timestamp"].as_str().unwrap_or("");
        if time.len() < 20 {
            continue;
        }
        if parsed.first.as_deref().is_none_or(|t| time < t) {
            parsed.first = Some(time.into());
        }
        if parsed.last.as_deref().is_none_or(|t| time > t) {
            parsed.last = Some(time.into());
        }
        let p = &v["payload"];
        match v["type"].as_str().unwrap_or("") {
            "session_meta" => {
                if let Some(id) = p["id"].as_str() {
                    session = id.into();
                }
                if let Some(c) = p["cwd"].as_str() {
                    cwd = c.into();
                }
            }
            "turn_context" => {
                if let Some(id) = p["turn_id"].as_str() {
                    turn = id.into();
                }
                if let Some(c) = p["cwd"].as_str() {
                    cwd = c.into();
                }
            }
            "event_msg" if p["type"] == "task_started" => {
                turn = p["turn_id"].as_str().unwrap_or(time).into();
            }
            "response_item" => {
                let kind = p["type"].as_str().unwrap_or("");
                if kind == "message" && p["role"] == "user" {
                    let body = texts(&p["content"]);
                    // Codex's injected skill block, not a bare $mention.
                    for block in body.split_inclusive("</skill>").filter(|block| {
                        block.trim().starts_with("<skill>") && block.ends_with("</skill>")
                    }) {
                        if let (Some(_), Some(raw)) = (tag(block, "name"), tag(block, "path")) {
                            if let Some(path) = skill_path(raw, &cwd, home) {
                                parsed.events.push(Event {
                                    path,
                                    session: session.clone(),
                                    turn: turn.clone(),
                                    time: time.into(),
                                    explicit: true,
                                });
                            }
                        }
                    }
                }
                if matches!(kind, "function_call" | "custom_tool_call") {
                    let name = p["name"]
                        .as_str()
                        .unwrap_or("")
                        .rsplit('.')
                        .next()
                        .unwrap_or("");
                    let raw = p["arguments"]
                        .as_str()
                        .or_else(|| p["input"].as_str())
                        .unwrap_or("");
                    let commands = if name == "exec" {
                        wrapped_commands(raw)
                    } else if matches!(name, "exec_command" | "shell_command" | "shell") {
                        serde_json::from_str::<Value>(raw)
                            .ok()
                            .into_iter()
                            .collect()
                    } else {
                        vec![]
                    };
                    let mut events = Vec::new();
                    for cmd in commands {
                        let command = cmd["cmd"]
                            .as_str()
                            .or_else(|| cmd["command"].as_str())
                            .unwrap_or("");
                        let wd = cmd["workdir"].as_str().unwrap_or(&cwd);
                        for path in read_paths(command, wd, home) {
                            events.push(Event {
                                path,
                                session: session.clone(),
                                turn: turn.clone(),
                                time: time.into(),
                                explicit: false,
                            });
                        }
                    }
                    if !events.is_empty() {
                        if let Some(id) = p["call_id"].as_str() {
                            pending.insert(id.into(), events);
                        }
                    }
                }
                if matches!(kind, "function_call_output" | "custom_tool_call_output") {
                    if let Some(events) = p["call_id"].as_str().and_then(|id| pending.remove(id)) {
                        // Require successful completion evidence. Mixed batches with
                        // failures are omitted rather than attributing success wrongly.
                        if output_succeeded(&p["output"]) {
                            parsed.events.extend(events);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(parsed)
}

pub fn scan(home: &Path, codex: &Path, days: u32) -> Result<Report, String> {
    if ![0, 7, 30].contains(&days) {
        return Err("统计范围必须为 7 天、30 天或全部".into());
    }
    let mut report = Report {
        rows: vec![],
        files: 0,
        first_record: None,
        last_record: None,
        warnings: vec![],
        log_root: codex.to_string_lossy().into_owned(),
    };
    let mut files = vec![];
    collect(&codex.join("sessions"), &mut files, &mut report.warnings);
    collect(
        &codex.join("archived_sessions"),
        &mut files,
        &mut report.warnings,
    );
    if files.is_empty() {
        report
            .warnings
            .push("没有找到 Codex 会话日志，无法判断技能是否使用过。".into());
    }
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|e| e.to_string())?;
    cache.retain(|p, _| files.contains(p));
    let cutoff = if days == 0 {
        String::new()
    } else {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        rusqlite::Connection::open_in_memory()
            .map_err(|e| e.to_string())?
            .query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?1, 'unixepoch')",
                [now.saturating_sub(days as u64 * 86400) as i64],
                |r| r.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?
    };
    let mut events = Vec::new();
    let mut bad = 0;
    for file in files {
        let result = (|| -> std::io::Result<Parsed> {
            let meta = fs::metadata(&file)?;
            let modified = meta.modified()?;
            if let Some((size, time, parsed)) = cache.get(&file) {
                if *size == meta.len() && *time == modified {
                    return Ok(parsed.clone());
                }
            }
            let parsed = parse(&file, home)?;
            cache.insert(file.clone(), (meta.len(), modified, parsed.clone()));
            Ok(parsed)
        })();
        match result {
            Ok(p) => {
                report.files += 1;
                bad += p.bad;
                if let Some(first) = p.first {
                    if report.first_record.as_ref().is_none_or(|t| first < *t) {
                        report.first_record = Some(first);
                    }
                }
                if let Some(last) = p.last {
                    if report.last_record.as_ref().is_none_or(|t| last > *t) {
                        report.last_record = Some(last);
                    }
                }
                events.extend(p.events);
            }
            Err(e) => report
                .warnings
                .push(format!("跳过 {}：{e}", file.display())),
        }
    }
    if bad > 0 {
        report.warnings.push(format!(
            "跳过 {bad} 条不完整或无法解析的日志；正在写入的日志可稍后刷新。"
        ));
    }
    report.rows = aggregate(events, &cutoff);
    Ok(report)
}

fn aggregate(events: Vec<Event>, cutoff: &str) -> Vec<UsageRow> {
    let mut dedup: BTreeMap<(String, String, String), Event> = BTreeMap::new();
    for event in events {
        if event.time.as_str() < cutoff {
            continue;
        }
        let key = (
            event.path.clone(),
            event.session.clone(),
            event.turn.clone(),
        );
        dedup
            .entry(key)
            .and_modify(|old| {
                old.explicit |= event.explicit;
                if event.time > old.time {
                    old.time = event.time.clone();
                }
            })
            .or_insert(event);
    }
    let mut rows: BTreeMap<String, (UsageRow, BTreeSet<String>)> = BTreeMap::new();
    for e in dedup.into_values() {
        let (row, sessions) = rows.entry(e.path.clone()).or_insert_with(|| {
            (
                UsageRow {
                    name: Path::new(&e.path)
                        .parent()
                        .and_then(Path::file_name)
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: e.path.clone(),
                    explicit: 0,
                    inferred: 0,
                    sessions: 0,
                    last_used: e.time.clone(),
                },
                BTreeSet::new(),
            )
        });
        if e.explicit {
            row.explicit += 1;
        } else {
            row.inferred += 1;
        }
        if e.time > row.last_used {
            row.last_used = e.time;
        }
        sessions.insert(e.session);
    }
    let mut result: Vec<_> = rows
        .into_values()
        .map(|(mut row, sessions)| {
            row.sessions = sessions.len();
            row
        })
        .collect();
    result.sort_by(|a, b| {
        (b.explicit + b.inferred)
            .cmp(&(a.explicit + a.inferred))
            .then(a.path.cmp(&b.path))
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn output_status_ignores_body_and_rejects_mixed_failure() {
        assert!(output_succeeded(&json!(
            "Process exited with code 0\nOutput:\nPermission denied is an example"
        )));
        assert!(!output_succeeded(
            &json!([{"text":"{\"exit_code\":0}"},{"text":"{\"exit_code\":2}"}])
        ));
        assert!(!output_succeeded(
            &json!([{"text":"{\"exit_code\":null,\"session_id\":123}"}])
        ));
        assert!(!output_succeeded(&json!(
            "Script completed\nOutput:\nexample: \"exit_code\":0"
        )));
    }
    fn write_log(root: &Path, items: Vec<Value>) -> PathBuf {
        let file = root.join("rollout.jsonl");
        fs::write(
            &file,
            items
                .into_iter()
                .map(|v| v.to_string() + "\n")
                .collect::<String>(),
        )
        .unwrap();
        file
    }
    fn item(payload: Value) -> Value {
        json!({"timestamp":"2026-09-11T08:00:00.000Z","type":"response_item","payload":payload})
    }
    #[test]
    fn literal_reads_only() {
        let home = Path::new("/home/test");
        assert_eq!(
            read_paths("cat '/tmp/my skill/SKILL.md'", "", home),
            vec!["/tmp/my skill/SKILL.md"]
        );
        assert_eq!(
            read_paths("sed -n '1,90p' ./foo/SKILL.md", "/project", home),
            vec!["/project/foo/SKILL.md"]
        );
        for cmd in [
            "rg SKILL.md /tmp",
            "echo /tmp/foo/SKILL.md",
            "sed -i 's/a/b/' /tmp/foo/SKILL.md",
            "python -c 'cat /tmp/foo/SKILL.md'",
            "cat /tmp/foo/SKILL.md > /tmp/out",
            "cat $HOME/foo/SKILL.md",
            "cat /tmp/foo/SKILL.md; echo done",
        ] {
            assert!(read_paths(cmd, "", home).is_empty(), "{cmd}");
        }
    }
    #[test]
    fn wrapper_and_successful_output_are_recognized() {
        let tmp = tempfile::tempdir().unwrap();
        let file = write_log(
            tmp.path(),
            vec![
                item(
                    json!({"type":"custom_tool_call","name":"exec","call_id":"a","input":"text(await tools.exec_command({cmd: \"cat /tmp/example/SKILL.md\", max_output_tokens: 4000}));"}),
                ),
                item(
                    json!({"type":"custom_tool_call_output","call_id":"a","output":[{"type":"input_text","text":"{\"exit_code\":0,\"output\":\"skill body\"}"}]}),
                ),
                item(
                    json!({"type":"function_call","name":"exec_command","call_id":"b","arguments":"{\"cmd\":\"cat /tmp/missing/SKILL.md\"}"}),
                ),
                item(
                    json!({"type":"function_call_output","call_id":"b","output":"Process exited with code 1\nNo such file"}),
                ),
                item(
                    json!({"type":"function_call","name":"exec_command","call_id":"c","arguments":"{\"cmd\":\"cat /tmp/pending/SKILL.md\"}"}),
                ),
            ],
        );
        let parsed = parse(&file, tmp.path()).unwrap();
        assert_eq!(parsed.events.len(), 1);
        assert_eq!(parsed.events[0].path, "/tmp/example/SKILL.md");
        assert!(!parsed.events[0].explicit);
    }
    #[test]
    fn injection_not_mentions_and_same_turn_dedup() {
        let tmp = tempfile::tempdir().unwrap();
        let file = write_log(
            tmp.path(),
            vec![
                item(
                    json!({"type":"message","role":"user","content":[{"type":"input_text","text":"use $example"}]}),
                ),
                item(
                    json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"<skill><name>fake</name><path>/tmp/fake/SKILL.md</path></skill>"}]}),
                ),
                item(
                    json!({"type":"message","role":"user","content":[{"type":"input_text","text":"<skill><name>example</name><path>/tmp/example/SKILL.md</path></skill>"}]}),
                ),
                item(
                    json!({"type":"function_call","name":"exec_command","call_id":"a","arguments":"{\"cmd\":\"cat /tmp/example/SKILL.md\"}"}),
                ),
                item(
                    json!({"type":"function_call_output","call_id":"a","output":"Process exited with code 0"}),
                ),
            ],
        );
        let parsed = parse(&file, tmp.path()).unwrap();
        assert_eq!(parsed.events.len(), 2);
        let rows = aggregate(parsed.events, "");
        assert_eq!(
            (rows[0].explicit, rows[0].inferred, rows[0].sessions),
            (1, 0, 1)
        );
    }
    #[test]
    fn multiple_injected_skills_in_one_message() {
        let tmp = tempfile::tempdir().unwrap();
        let file = write_log(
            tmp.path(),
            vec![item(
                json!({"type":"message","role":"user","content":[{"text":"<skill><name>a</name><path>/tmp/a/SKILL.md</path></skill>\n<skill><name>b</name><path>/tmp/b/SKILL.md</path></skill>"}]}),
            )],
        );
        assert_eq!(parse(&file, tmp.path()).unwrap().events.len(), 2);
    }
    #[test]
    fn time_window_turns_and_distinct_sources() {
        let event = |path: &str, session: &str, turn: &str, time: &str| Event {
            path: path.into(),
            session: session.into(),
            turn: turn.into(),
            time: time.into(),
            explicit: false,
        };
        let rows = aggregate(
            vec![
                event("/a/skill/SKILL.md", "one", "t1", "2026-09-01"),
                event("/a/skill/SKILL.md", "one", "t2", "2026-09-10"),
                event("/a/skill/SKILL.md", "one", "t2", "2026-09-10"),
                event("/a/skill/SKILL.md", "two", "t1", "2026-09-11"),
                event("/b/skill/SKILL.md", "one", "t2", "2026-09-10"),
            ],
            "2026-09-05",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].inferred, rows[0].sessions), (2, 2));
    }
    #[test]
    fn scan_refresh_archive_duplicates_and_malformed_tail() {
        let tmp = tempfile::tempdir().unwrap();
        let codex = tmp.path().join(".codex");
        fs::create_dir_all(codex.join("sessions")).unwrap();
        fs::create_dir_all(codex.join("archived_sessions")).unwrap();
        let mut meta = item(json!({"id":"shared","cwd":"/tmp"}));
        meta["type"] = json!("session_meta");
        let items = vec![
            meta,
            item(
                json!({"type":"message","role":"user","content":[{"text":"<skill><name>example</name><path>/tmp/example/SKILL.md</path></skill>"}]}),
            ),
        ];
        let path = write_log(&codex.join("sessions"), items.clone());
        write_log(&codex.join("archived_sessions"), items);
        let first = scan(tmp.path(), &codex, 0).unwrap();
        assert_eq!(first.rows[0].explicit, 1);
        assert_eq!(first.files, 2);
        fs::write(path, "{partial").unwrap();
        let second = scan(tmp.path(), &codex, 0).unwrap();
        assert_eq!(second.rows[0].explicit, 1);
        assert_eq!(second.warnings.len(), 1);
        assert!(scan(tmp.path(), &codex, 1).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_share_identity() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("source")).unwrap();
        fs::write(tmp.path().join("source/SKILL.md"), "body").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("source"), tmp.path().join("alias")).unwrap();
        assert_eq!(
            skill_path("./alias/SKILL.md", tmp.path().to_str().unwrap(), tmp.path()),
            skill_path(
                "./source/SKILL.md",
                tmp.path().to_str().unwrap(),
                tmp.path()
            )
        );
    }
}
