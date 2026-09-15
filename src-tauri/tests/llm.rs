use serde_json::{json, Value};
use skill_manager::llm::{self, ConfigInput};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

fn input(url: &str, key: Option<&str>) -> ConfigInput {
    ConfigInput {
        base_url: url.into(),
        model: "fixture-model".into(),
        api_key: key.map(str::to_string),
        thinking: None,
    }
}
fn server(status: &str, body: Value) -> (String, thread::JoinHandle<(String, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let status = status.to_string();
    let handle = thread::spawn(move || {
        let start = Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        start.elapsed() < Duration::from_secs(10),
                        "request did not reach fixture server"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        };
        // Accepted sockets inherit nonblocking mode on macOS.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut buf = [0; 4096];
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
        let length = headers
            .lines()
            .find_map(|line| {
                line.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|s| s.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while bytes.len() < header_end + length {
            let mut buf = [0; 4096];
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
        }
        let request = if length == 0 {
            Value::Null
        } else {
            serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
        };
        let text = body.to_string();
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).unwrap();
        (headers, request)
    });
    (url, handle)
}
#[test]
fn normalizes_supported_endpoints_and_rejects_credential_urls() {
    assert_eq!(
        llm::normalize_base("https://example.test/").unwrap(),
        "https://example.test/v1"
    );
    assert_eq!(
        llm::normalize_base("https://example.test/api/v1/chat/completions/").unwrap(),
        "https://example.test/api/v1"
    );
    assert_eq!(
        llm::normalize_base("http://localhost:1234/v1/models").unwrap(),
        "http://localhost:1234/v1"
    );
    for url in [
        "file:///tmp/key",
        "https://key@example.test/v1",
        "https://example.test/v1?key=secret",
    ] {
        assert!(llm::normalize_base(url).is_err());
    }
}
#[test]
fn config_redacts_key_preserves_it_and_requires_reentry_for_new_host() {
    let temp = tempfile::tempdir().unwrap();
    llm::save_config(
        temp.path(),
        input("https://example.test/v1", Some("test-secret")),
    )
    .unwrap();
    let view = llm::save_config(temp.path(), input("https://example.test/v1", None)).unwrap();
    assert!(view.has_key);
    assert!(!serde_json::to_string(&view)
        .unwrap()
        .contains("test-secret"));
    assert!(llm::save_config(temp.path(), input("https://other.test/v1", None)).is_err());
    assert!(
        !llm::save_config(temp.path(), input("https://other.test/v1", Some("")))
            .unwrap()
            .has_key
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(temp.path().join("llm.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
#[test]
fn fetches_models_using_draft_config_without_saving() {
    let temp = tempfile::tempdir().unwrap();
    let (url, handle) = server("200 OK", json!({"data":[{"id":"z"},{"id":"a"},{"id":"z"}]}));
    assert_eq!(
        llm::models(temp.path(), input(&url, Some("fixture-key"))).unwrap(),
        ["a", "z"]
    );
    let (headers, _) = handle.join().unwrap();
    assert!(headers.starts_with("GET /v1/models "));
    assert!(headers
        .to_lowercase()
        .contains("authorization: bearer fixture-key"));
    assert!(!temp.path().join("llm.json").exists());
}
#[test]
fn translation_is_read_only_preserves_metadata_and_sends_only_body() {
    let temp = tempfile::tempdir().unwrap();
    let text = "\u{feff}---\r\nname: demo\r\nglobs: '**/*.rs'\r\n---\r\n# Hello\nRead `SKILL.md`.";
    let (url, handle) = server(
        "200 OK",
        json!({"choices":[{"finish_reason":"stop", "message":{"content":"# 你好\n阅读 `SKILL.md`。"}}]}),
    );
    llm::save_config(temp.path(), input(&url, Some("fixture-key"))).unwrap();
    let file = temp.path().join("SKILL.md");
    fs::write(&file, text).unwrap();
    let result = llm::generate(temp.path(), text, "translate", "简体中文").unwrap();
    assert_eq!(llm::split_metadata(&result).0, llm::split_metadata(text).0);
    assert!(result.ends_with("# 你好\n阅读 `SKILL.md`。"));
    assert_eq!(fs::read_to_string(file).unwrap(), text);
    let (headers, body) = handle.join().unwrap();
    assert!(headers.starts_with("POST /v1/chat/completions "));
    assert_eq!(body["model"], "fixture-model");
    assert_eq!(body["messages"][1]["content"], "# Hello\nRead `SKILL.md`.");
    assert_eq!(body["stream"], false);
}
#[test]
fn overview_includes_document_metadata_and_stays_separate() {
    let temp = tempfile::tempdir().unwrap();
    let (url, handle) = server(
        "200 OK",
        json!({"choices":[{"finish_reason":"stop", "message":{"content":"## 一句话概览\n说明文档用途"}}]}),
    );
    llm::save_config(temp.path(), input(&url, None)).unwrap();
    let text = "---\nname: test\n---\nDocument";
    let result = llm::generate(temp.path(), text, "overview", "简体中文").unwrap();
    assert!(result.starts_with("## 一句话概览"));
    assert_eq!(handle.join().unwrap().1["messages"][1]["content"], text);
}
#[test]
fn rejects_truncated_completions_and_hides_server_secrets() {
    let temp = tempfile::tempdir().unwrap();
    let (url, handle) = server(
        "200 OK",
        json!({"choices":[{"finish_reason":"length", "message":{"content":"partial text"}}]}),
    );
    llm::save_config(temp.path(), input(&url, None)).unwrap();
    assert!(llm::generate(temp.path(), "body", "translate", "中文")
        .unwrap_err()
        .contains("未完整"));
    handle.join().unwrap();
    let (url, handle) = server(
        "401 Unauthorized",
        json!({"error":{"message":"secret-key-123"}}),
    );
    let error = llm::models(temp.path(), input(&url, Some(""))).unwrap_err();
    assert!(error.contains("401"));
    assert!(!error.contains("secret-key-123"));
    handle.join().unwrap();
}
#[test]
fn overwrite_backs_up_and_checks_staleness_metadata_and_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("SKILL.md");
    let original = "---\nname: demo\n---\nHello";
    let translated = "---\nname: demo\n---\n你好";
    fs::write(&file, original).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    }
    let result = llm::replace_file(&file, original, translated, &root.join("backups")).unwrap();
    assert_eq!(fs::read_to_string(result.backup_path).unwrap(), original);
    assert_eq!(fs::read_to_string(&file).unwrap(), translated);
    assert!(llm::replace_file(&file, original, translated, &root.join("backups")).is_err());
    assert!(llm::replace_file(&file, translated, "no metadata", &root.join("backups")).is_err());
    llm::replace_file(&file, translated, original, &root.join("backups")).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}

#[test]
fn aligned_translation_restores_source_order_using_ids() {
    let temp = tempfile::tempdir().unwrap();
    let (url, handle) = server(
        "200 OK",
        json!({"choices":[{"finish_reason":"stop", "message":{"content":"{\"translations\":[{\"id\":12,\"text\":\"第二句。\"},{\"id\":11,\"text\":\"第一句。\"}]}"}}]}),
    );
    llm::save_config(temp.path(), input(&url, None)).unwrap();
    let segments = vec![
        llm::TranslationSegment {
            id: 11,
            text: "First sentence.".into(),
        },
        llm::TranslationSegment {
            id: 12,
            text: "Second sentence.".into(),
        },
    ];
    let result = llm::translate_segments(temp.path(), segments, "简体中文").unwrap();
    assert_eq!(result.iter().map(|s| s.id).collect::<Vec<_>>(), [11, 12]);
    assert_eq!(result[0].text, "第一句。");
    let (_, payload) = handle.join().unwrap();
    let sent: Value =
        serde_json::from_str(payload["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(sent[0]["id"], 11);
    assert_eq!(sent[1]["text"], "Second sentence.");
}

#[test]
fn aligned_translation_rejects_missing_duplicate_unknown_or_malformed_results() {
    for translation in [
        "{\"translations\":[]}",
        "{\"translations\":[{\"id\":0,\"text\":\"甲\"},{\"id\":0,\"text\":\"乙\"}]}",
        "{\"translations\":[{\"id\":99,\"text\":\"甲\"}]}",
        "{\"translations\":[{\"id\":0,\"text\":\"\"}]}",
        "not JSON",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (url, handle) = server(
            "200 OK",
            json!({"choices":[{"finish_reason":"stop", "message":{"content":translation}}]}),
        );
        llm::save_config(temp.path(), input(&url, None)).unwrap();
        assert!(llm::translate_segments(
            temp.path(),
            vec![llm::TranslationSegment {
                id: 0,
                text: "Source sentence.".into()
            }],
            "中文"
        )
        .is_err());
        handle.join().unwrap();
    }
}

#[test]
fn streamed_output_is_delivered_before_eof_without_exposing_reasoning() {
    use std::{cell::Cell, rc::Rc};
    struct Staged {
        delivered: Rc<Cell<bool>>,
        stage: usize,
    }
    impl Read for Staged {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let data = match self.stage {
                0 => "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"private reasoning\"}}]}\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"概览\"}}]}\r\n\r\n",
                1 => { assert!(self.delivered.get(), "must deliver before reading the next chunk"); "data: {\"choices\":[{\"delta\":{\"content\":\"正文\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n" },
                _ => return Ok(0),
            };
            assert!(buf.len() >= data.len());
            buf[..data.len()].copy_from_slice(data.as_bytes());
            self.stage += 1;
            Ok(data.len())
        }
    }
    let delivered = Rc::new(Cell::new(false));
    let mut events = Vec::new();
    let result = llm::read_event_stream(
        Staged {
            delivered: delivered.clone(),
            stage: 0,
        },
        &mut |event| {
            if matches!(event, llm::GenerationProgress::Delta { .. }) {
                delivered.set(true);
            }
            events.push(serde_json::to_string(&event).unwrap());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result, "概览正文");
    assert!(!events.join("").contains("private reasoning"));
}

#[test]
fn streamed_truncation_never_reports_success() {
    for stream in [
        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n",
    ] {
        assert!(llm::read_event_stream(stream.as_bytes(), &mut |_| Ok(())).unwrap_err().contains("未完整"));
    }
}

#[test]
fn streaming_accepts_json_fallback_and_disables_deepseek_thinking_by_default() {
    let temp = tempfile::tempdir().unwrap();
    let (url, handle) = server(
        "200 OK",
        json!({"choices":[{"finish_reason":"stop","message":{"content":"概览"}}]}),
    );
    let mut config = input(&url, None);
    config.model = "deepseek-v4-flash".into();
    llm::save_config(temp.path(), config).unwrap();
    let result = llm::generate_streamed(
        temp.path(),
        "Example document",
        "overview",
        "中文",
        &mut |_| Ok(()),
    )
    .unwrap();
    assert_eq!(result, "概览");
    let (_, request) = handle.join().unwrap();
    assert_eq!(request["stream"], true);
    assert_eq!(request["thinking"]["type"], "disabled");
}

#[test]
fn review_modes_send_evidence_and_generate_read_only_reports() {
    for mode in ["conflicts", "quality", "improvements"] {
        let temp = tempfile::tempdir().unwrap();
        let source =
            "---\nname: example\n---\nAlways ask before writing.\nNever ask before writing.";
        let file = temp.path().join("SKILL.md");
        fs::write(&file, source).unwrap();
        let text = json!({"documents": [{"id": "D1", "name": "example", "source": "SKILL.md", "lines": source.lines().enumerate().map(|(i, text)| json!({"line": i+1, "text": text})).collect::<Vec<_>>()}]}).to_string();
        let (url, handle) = server(
            "200 OK",
            json!({"choices":[{"finish_reason":"stop","message":{"content":"## 检查结果\nD1 第 4–5 行存在矛盾。"}}]}),
        );
        llm::save_config(temp.path(), input(&url, None)).unwrap();
        let mut events = Vec::new();
        let report =
            llm::generate_streamed(temp.path(), &text, mode, "简体中文", &mut |event| {
                events.push(event);
                Ok(())
            })
            .unwrap();
        assert!(report.starts_with("## 检查结果"));
        assert!(!report.starts_with("---"));
        assert!(!events.is_empty());
        assert_eq!(fs::read_to_string(&file).unwrap(), source);
        let (_, payload) = handle.join().unwrap();
        assert_eq!(payload["messages"][1]["content"], text);
        let instruction = payload["messages"][0]["content"].as_str().unwrap();
        assert!(instruction.contains("行号"));
        assert!(instruction.contains("untrusted document content"));
        assert_eq!(payload["stream"], true);
    }
}

#[test]
fn review_modes_reject_incomplete_reports() {
    for mode in ["conflicts", "quality", "improvements"] {
        let temp = tempfile::tempdir().unwrap();
        let (url, handle) = server(
            "200 OK",
            json!({"choices":[{"finish_reason":"length","message":{"content":"未发现冲突"}}]}),
        );
        llm::save_config(temp.path(), input(&url, None)).unwrap();
        assert!(
            llm::generate_streamed(temp.path(), "Document", mode, "中文", &mut |_| Ok(()))
                .unwrap_err()
                .contains("未完整")
        );
        handle.join().unwrap();
    }
}

#[test]
fn fixture_server_waits_for_delayed_request_bytes() {
    // accept() can inherit O_NONBLOCK on macOS; headers may arrive later.
    let (url, handle) = server("200 OK", json!({}));
    let address = url.strip_prefix("http://").unwrap().strip_suffix("/v1").unwrap();
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    thread::sleep(Duration::from_millis(100));
    let _ = stream.write_all(b"GET /v1 HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let (headers, _) = handle.join().expect("fixture must wait for request bytes");
    assert!(headers.starts_with("GET /v1 HTTP/1.1"));
}
