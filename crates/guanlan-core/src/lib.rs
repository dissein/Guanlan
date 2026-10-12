pub mod agent;
pub mod context;
pub mod stream;
pub mod tools;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub workspace: String,
    pub base_url: String,
    pub model: String,
    pub mode: String,
    pub auto_approve: bool,
    pub max_steps: usize,
    pub context_budget: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            workspace: String::new(),
            base_url: "https://api.deepseek.com/v1".into(),
            model: String::new(),
            mode: "build".into(),
            auto_approve: false,
            max_steps: 30,
            context_budget: 120_000,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct UndoRecord {
    pub files: BTreeMap<String, Option<Vec<u8>>>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub messages: Vec<Value>,
    #[serde(default)]
    pub undo: Vec<UndoRecord>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title: "新对话".into(),
            messages: vec![],
            undo: vec![],
        }
    }
}
pub fn root(path: &str) -> Result<PathBuf> {
    if path.trim().is_empty() {
        bail!("请先选择工作目录")
    }
    let p = fs::canonicalize(path).context("工作目录不存在")?;
    if !p.is_dir() {
        bail!("工作目录必须是文件夹")
    }
    Ok(p)
}
/// Resolve even a new file through its existing ancestor, rejecting traversal and symlink escapes.
pub fn safe_path(root: &Path, input: &str) -> Result<PathBuf> {
    let p = Path::new(input);
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    };
    let mut clean = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => (),
            Component::ParentDir => {
                clean.pop();
            }
            _ => clean.push(c.as_os_str()),
        }
    }
    let mut ancestor = clean.as_path();
    let mut tail = Vec::new();
    while fs::symlink_metadata(ancestor).is_err() {
        tail.push(ancestor.file_name().context("无效路径")?.to_os_string());
        ancestor = ancestor.parent().context("无效路径")?;
    }
    let mut resolved = fs::canonicalize(ancestor)?;
    for part in tail.iter().rev() {
        resolved.push(part);
    }
    if !resolved.starts_with(root) {
        bail!("路径超出当前工作目录")
    }
    let relative = resolved.strip_prefix(root)?;
    if relative.components().any(|x| x.as_os_str() == ".guanlan") {
        bail!("不能访问观澜内部会话目录")
    }
    Ok(resolved)
}
pub fn save_session(root: &Path, session: &Session) -> Result<()> {
    Uuid::parse_str(&session.id).context("无效会话编号")?;
    let dir = session_dir(root)?;
    let path = dir.join(format!("{}.json", session.id));
    // A temporary file prevents a partial JSON document if interrupted while writing.
    let temp = dir.join(format!("{}.tmp", Uuid::new_v4()));
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(session)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)?;
    Ok(())
}
pub fn list_sessions(root: &Path) -> Result<Vec<Value>> {
    let dir = root.join(".guanlan/sessions");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let dir = session_dir(root)?;
    let mut items: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|x| x == "json")
                && e.file_type().is_ok_and(|t| t.is_file())
        })
        .collect();
    items.sort_by_key(|e| std::cmp::Reverse(e.metadata().and_then(|m| m.modified()).ok()));
    Ok(items
        .iter()
        .filter_map(|e| {
            fs::read(e.path())
                .ok()
                .and_then(|b| serde_json::from_slice::<Session>(&b).ok())
        })
        .map(|s| json!({"id":s.id,"title":s.title}))
        .collect())
}
pub fn load_session(root: &Path, id: &str) -> Result<Session> {
    Uuid::parse_str(id).context("无效会话编号")?;
    let dir = session_dir(root)?;
    let path = fs::canonicalize(dir.join(format!("{id}.json")))?;
    if !path.starts_with(&dir) {
        bail!("会话路径超出内部目录")
    }
    let mut s: Session = serde_json::from_slice(&fs::read(path)?)?;
    repair_messages(&mut s.messages);
    Ok(s)
}
fn session_dir(root: &Path) -> Result<PathBuf> {
    let internal = root.join(".guanlan");
    fs::create_dir_all(&internal)?;
    let internal = fs::canonicalize(internal)?;
    if !internal.starts_with(root) {
        bail!("会话目录不能指向工作区之外")
    };
    let dir = internal.join("sessions");
    fs::create_dir_all(&dir)?;
    let dir = fs::canonicalize(dir)?;
    if !dir.starts_with(&internal) {
        bail!("会话目录不能使用外部链接")
    };
    Ok(dir)
}
pub fn repair_messages(messages: &mut Vec<Value>) {
    let mut repaired = Vec::new();
    let mut pending: Vec<String> = vec![];
    for m in messages.drain(..) {
        let role = m["role"].as_str().unwrap_or("");
        if role != "tool" {
            for id in pending.drain(..) {
                repaired.push(json!({"role":"tool","tool_call_id":id,"content":"工具执行中断。"}));
            }
        }
        if role == "tool" {
            let id = m["tool_call_id"].as_str().unwrap_or("");
            if let Some(i) = pending.iter().position(|p| p == id) {
                pending.remove(i);
            } else {
                continue;
            }
        }
        if let Some(calls) = m["tool_calls"].as_array() {
            pending = calls
                .iter()
                .filter_map(|c| c["id"].as_str().map(str::to_owned))
                .collect();
        }
        repaired.push(m);
    }
    for id in pending {
        repaired.push(json!({"role":"tool","tool_call_id":id,"content":"工具执行中断。"}));
    }
    *messages = repaired;
}
pub fn undo(root: &Path, session: &mut Session) -> Result<usize> {
    let record = session.undo.last().context("没有可撤销的文件修改")?;
    // Validate every path before the first mutation.
    let paths: Vec<_> = record
        .files
        .keys()
        .map(|p| safe_path(root, p))
        .collect::<Result<_>>()?;
    for (path, content) in paths.iter().zip(record.files.values()) {
        if let Some(bytes) = content {
            if let Some(p) = path.parent() {
                fs::create_dir_all(p)?;
            }
            fs::write(path, bytes)?;
        } else if path.exists() {
            fs::remove_file(path)?;
        }
    }
    let count = record.files.len();
    session.undo.pop();
    save_session(root, session)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_traversal_and_internal_paths() {
        let dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).unwrap();
        let r = fs::canonicalize(&dir).unwrap();
        assert!(safe_path(&r, "../escape.txt").is_err());
        assert!(safe_path(&r, ".guanlan/sessions/x").is_err());
        assert!(safe_path(&r, "new/deep.txt").unwrap().starts_with(&r));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn interrupted_tool_transactions_repaired() {
        let mut m = vec![
            json!({"role":"assistant","tool_calls":[{"id":"a"},{"id":"b"}]}),
            json!({"role":"tool","tool_call_id":"a","content":"ok"}),
            json!({"role":"user","content":"next"}),
        ];
        repair_messages(&mut m);
        assert_eq!(m[2]["tool_call_id"], "b");
        assert_eq!(m[3]["role"], "user");
    }
}
