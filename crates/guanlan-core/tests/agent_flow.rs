use guanlan_core::{
    agent::{run, Approvals},
    root, Session, Settings,
};
use serde_json::{json, Value};
use std::{fs, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

async fn mock(rounds: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut requests = vec![];
        for response in rounds {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![];
            let mut chunk = [0u8; 2048];
            loop {
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(pos) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..pos]).to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if bytes.len() >= pos + 4 + length {
                        requests.push(
                            serde_json::from_slice(&bytes[pos + 4..pos + 4 + length]).unwrap(),
                        );
                        break;
                    }
                }
            }
            let header=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",response.len());
            socket.write_all(header.as_bytes()).await.unwrap();
            for fragment in response.as_bytes().chunks(7) {
                socket.write_all(fragment).await.unwrap();
                tokio::task::yield_now().await;
            }
            socket.shutdown().await.unwrap();
        }
        requests
    });
    (format!("http://{addr}/v1"), handle)
}
fn call(name: &str, args: Value) -> String {
    format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":name,"arguments":args.to_string()}}]}}]}),
        json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]})
    )
}
fn answer() -> String {
    format!(
        "data: {}\r\n\r\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"content":"完成了。"}}]}),
        json!({"choices":[{"delta":{},"finish_reason":"stop"}]})
    )
}
fn workspace() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&p).unwrap();
    p
}

#[tokio::test]
async fn failed_shell_is_an_error_in_events_and_model_context() {
    let dir = workspace();
    let (base, server) = mock(vec![call("bash", json!({"command":"exit 3"})), answer()]).await;
    let settings = Settings {
        workspace: dir.to_string_lossy().into(),
        base_url: base,
        auto_approve: true,
        ..Settings::default()
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
    let mut session = Session::default();
    run(
        &settings,
        "",
        "检查失败状态".into(),
        &mut session,
        Arc::new(Approvals::default()),
        tx,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let mut failure_seen = false;
    while let Some(e) = rx.recv().await {
        if e["type"] == "tool_finish" {
            assert_eq!(e["ok"], false);
            failure_seen = true;
        }
    }
    assert!(failure_seen);
    let requests = server.await.unwrap();
    assert!(requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "tool" && m["content"].as_str().unwrap_or("").starts_with("ERROR")));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn approved_write_followed_by_reply_and_durable_undo() {
    let dir = workspace();
    let (base, server) = mock(vec![
        call("write", json!({"path":"hello.txt","content":"你好"})),
        answer(),
    ])
    .await;
    let settings = Settings {
        workspace: dir.to_string_lossy().into(),
        base_url: base,
        ..Settings::default()
    };
    let a = Arc::new(Approvals::default());
    let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
    let resolver = a.clone();
    let events = tokio::spawn(async move {
        let mut seen = vec![];
        while let Some(e) = rx.recv().await {
            if e["type"] == "approval" {
                resolver
                    .resolve(e["id"].as_str().unwrap(), "approve".into())
                    .await
                    .unwrap();
            }
            seen.push(e);
        }
        seen
    });
    let mut session = Session::default();
    run(
        &settings,
        "",
        "写一个文件".into(),
        &mut session,
        a,
        tx,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let seen = events.await.unwrap();
    assert!(seen.iter().any(|e| e["type"] == "change"));
    assert_eq!(fs::read_to_string(dir.join("hello.txt")).unwrap(), "你好");
    assert_eq!(session.messages.last().unwrap()["content"], "完成了。");
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["messages"][3]["role"], "tool");
    let r = root(&settings.workspace).unwrap();
    let mut loaded = guanlan_core::load_session(&r, &session.id).unwrap();
    guanlan_core::undo(&r, &mut loaded).unwrap();
    assert!(!dir.join("hello.txt").exists());
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn plan_mode_rejects_write_even_with_auto_approve() {
    let dir = workspace();
    let (base, server) = mock(vec![
        call("write", json!({"path":"no.txt","content":"x"})),
        answer(),
    ])
    .await;
    let settings = Settings {
        workspace: dir.to_string_lossy().into(),
        base_url: base,
        mode: "plan".into(),
        auto_approve: true,
        ..Settings::default()
    };
    let (tx, _rx) = mpsc::unbounded_channel::<Value>();
    let mut s = Session::default();
    run(
        &settings,
        "",
        "plan".into(),
        &mut s,
        Arc::new(Approvals::default()),
        tx,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!dir.join("no.txt").exists());
    assert!(s.messages[2]["content"]
        .as_str()
        .unwrap()
        .contains("规划模式"));
    server.await.unwrap();
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn cancel_while_waiting_for_permission_never_writes() {
    let dir = workspace();
    let (base, server) = mock(vec![call("write", json!({"path":"no.txt","content":"x"}))]).await;
    let settings = Settings {
        workspace: dir.to_string_lossy().into(),
        base_url: base,
        ..Settings::default()
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
    let c = CancellationToken::new();
    let stop = c.clone();
    let observer = tokio::spawn(async move {
        while let Some(e) = rx.recv().await {
            if e["type"] == "approval" {
                stop.cancel();
            }
        }
    });
    let mut s = Session::default();
    assert!(run(
        &settings,
        "",
        "write".into(),
        &mut s,
        Arc::new(Approvals::default()),
        tx,
        c
    )
    .await
    .is_err());
    observer.await.unwrap();
    server.await.unwrap();
    assert!(!dir.join("no.txt").exists());
    assert_eq!(s.messages.last().unwrap()["role"], "tool");
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn interrupted_stream_does_not_execute_partial_tool_call() {
    let dir = workspace();
    let response = format!(
        "data: {}\n\n",
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"x","function":{"name":"write","arguments":json!({"path":"no.txt","content":"x"}).to_string()}}]}}]})
    );
    let (base, server) = mock(vec![response]).await;
    let settings = Settings {
        workspace: dir.to_string_lossy().into(),
        base_url: base,
        auto_approve: true,
        ..Settings::default()
    };
    let (tx, _rx) = mpsc::unbounded_channel::<Value>();
    let mut s = Session::default();
    assert!(run(
        &settings,
        "",
        "write".into(),
        &mut s,
        Arc::new(Approvals::default()),
        tx,
        CancellationToken::new()
    )
    .await
    .is_err());
    server.await.unwrap();
    assert!(!dir.join("no.txt").exists());
    assert_eq!(s.messages.len(), 1);
    fs::remove_dir_all(dir).unwrap();
}
