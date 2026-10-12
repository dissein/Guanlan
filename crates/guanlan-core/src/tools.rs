use crate::{safe_path, UndoRecord};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use walkdir::WalkDir;

pub fn schemas() -> Vec<Value> {
    let specs = vec![
        (
            "read",
            "读取工作区文本文件，带行号",
            json!({"path":{"type":"string"},"offset":{"type":"integer"},"limit":{"type":"integer"}}),
            vec!["path"],
        ),
        (
            "write",
            "写入文本文件，覆盖前需审批",
            json!({"path":{"type":"string"},"content":{"type":"string"}}),
            vec!["path", "content"],
        ),
        (
            "edit",
            "精确替换文本；非唯一匹配需要 replaceAll",
            json!({"path":{"type":"string"},"oldString":{"type":"string"},"newString":{"type":"string"},"replaceAll":{"type":"boolean"}}),
            vec!["path", "oldString", "newString"],
        ),
        (
            "apply_patch",
            "应用 *** Begin Patch 格式的 Add/Update/Delete File 补丁",
            json!({"patch":{"type":"string"}}),
            vec!["patch"],
        ),
        (
            "list_dir",
            "列出工作区文件",
            json!({"path":{"type":"string"},"recursive":{"type":"boolean"},"maxdepth":{"type":"integer"}}),
            vec![],
        ),
        (
            "glob",
            "按 glob 表达式查找文件，例如 **/*.rs",
            json!({"pattern":{"type":"string"},"path":{"type":"string"}}),
            vec!["pattern"],
        ),
        (
            "grep",
            "用正则表达式搜索文本",
            json!({"pattern":{"type":"string"},"path":{"type":"string"},"include":{"type":"string"}}),
            vec!["pattern"],
        ),
        (
            "bash",
            "在工作区运行本地命令；Windows 使用 PowerShell，必须先审批",
            json!({"command":{"type":"string"},"workdir":{"type":"string"},"timeout":{"type":"integer"}}),
            vec!["command"],
        ),
        (
            "todowrite",
            "更新当前任务清单",
            json!({"todos":{"type":"array","items":{"type":"object","properties":{"content":{"type":"string"},"status":{"type":"string","enum":["pending","in_progress","completed"]}},"required":["content","status"]}}}),
            vec!["todos"],
        ),
        (
            "question",
            "向用户提问并等待文字回复",
            json!({"question":{"type":"string"}}),
            vec!["question"],
        ),
        (
            "skill",
            "读取工作区 .agents/skills/<name>/SKILL.md 或 .guanlan-skills/<name>/SKILL.md",
            json!({"name":{"type":"string"}}),
            vec!["name"],
        ),
        (
            "websearch",
            "通过 DuckDuckGo HTML 搜索互联网；返回标题与链接，服务不可用时明确报错",
            json!({"query":{"type":"string"},"numResults":{"type":"integer"}}),
            vec!["query"],
        ),
        (
            "webfetch",
            "抓取 HTTP(S) 页面或文档，返回文本，最多 2 MB",
            json!({"url":{"type":"string"}}),
            vec!["url"],
        ),
    ];
    specs.into_iter().map(|(name,desc,properties,required)|json!({"type":"function","function":{"name":name,"description":desc,"parameters":{"type":"object","properties":properties,"required":required}}})).collect()
}
pub fn mutating(name: &str) -> bool {
    matches!(name, "write" | "edit" | "apply_patch" | "bash")
}
pub struct Tools {
    pub root: PathBuf,
    pub snapshots: BTreeMap<String, Option<Vec<u8>>>,
    pub changes: Vec<Value>,
}
impl Tools {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            snapshots: BTreeMap::new(),
            changes: vec![],
        }
    }
    fn snapshot(&mut self, path: &Path) -> Result<()> {
        let key = path.to_string_lossy().into_owned();
        if let std::collections::btree_map::Entry::Vacant(entry) = self.snapshots.entry(key) {
            let old = if path.exists() {
                let b = fs::read(path)?;
                if b.len() > 8 * 1024 * 1024 {
                    bail!("文件超过 8 MB 修改上限")
                }
                Some(b)
            } else {
                None
            };
            entry.insert(old);
        }
        Ok(())
    }
    fn write(&mut self, path: &Path, content: &str) -> Result<String> {
        if content.len() > 8 * 1024 * 1024 {
            bail!("内容超过 8 MB 修改上限")
        }
        let before = if path.exists() {
            fs::read_to_string(path).context("仅支持 UTF-8 文本修改")?
        } else {
            String::new()
        };
        self.snapshot(path)?;
        if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(path, content)?;
        let relative = path.strip_prefix(&self.root)?.to_string_lossy();
        let diff = similar::TextDiff::from_lines(before.as_str(), content)
            .unified_diff()
            .header(&format!("a/{relative}"), &format!("b/{relative}"))
            .to_string();
        self.changes.push(json!({"path":relative,"diff":diff}));
        Ok(diff)
    }
    pub fn record(&self) -> UndoRecord {
        UndoRecord {
            files: self.snapshots.clone(),
        }
    }
    pub async fn execute(
        &mut self,
        name: &str,
        a: &Value,
        cancel: &CancellationToken,
    ) -> Result<String> {
        match name {
            "read" => {
                let p = safe_path(&self.root, s(a, "path")?)?;
                if fs::metadata(&p)?.len() > 8 * 1024 * 1024 {
                    bail!("文件过大，请缩小读取范围或使用 grep")
                };
                let text = fs::read_to_string(p).context("仅支持 UTF-8 文本")?;
                let offset = a["offset"].as_u64().unwrap_or(1).max(1) as usize;
                let limit = a["limit"].as_u64().unwrap_or(800).min(2000) as usize;
                Ok(text
                    .lines()
                    .enumerate()
                    .skip(offset - 1)
                    .take(limit)
                    .map(|(i, l)| format!("{} | {}", i + 1, l))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "write" => {
                let p = safe_path(&self.root, s(a, "path")?)?;
                self.write(&p, s(a, "content")?)
            }
            "edit" => {
                let p = safe_path(&self.root, s(a, "path")?)?;
                let old = fs::read_to_string(&p)?;
                let from = s(a, "oldString")?;
                let to = s(a, "newString")?;
                if from.is_empty() {
                    bail!("oldString 不能为空")
                };
                let count = old.matches(from).count();
                if count == 0 {
                    bail!("未找到指定文本")
                };
                let all = a["replaceAll"].as_bool().unwrap_or(false);
                if count > 1 && !all {
                    bail!("匹配多处，请提供更多上下文或 replaceAll=true")
                };
                let next = if all {
                    old.replace(from, to)
                } else {
                    old.replacen(from, to, 1)
                };
                self.write(&p, &next)
            }
            "apply_patch" => self.patch(s(a, "patch")?),
            "list_dir" | "glob" | "grep" => self.search(name, a),
            "bash" => {
                let dir = safe_path(&self.root, a["workdir"].as_str().unwrap_or("."))?;
                shell(
                    s(a, "command")?,
                    &dir,
                    a["timeout"].as_u64().unwrap_or(60).clamp(1, 600),
                    cancel,
                )
                .await
            }
            "todowrite" => Ok(serde_json::to_string(&a["todos"])?),
            "skill" => {
                let name = s(a, "name")?;
                if name.contains(['/', '\\']) || name == ".." {
                    bail!("无效技能名")
                };
                for folder in [".agents/skills", ".guanlan-skills"] {
                    let p = safe_path(&self.root, &format!("{folder}/{name}/SKILL.md"))?;
                    if p.exists() {
                        return Ok(fs::read_to_string(p)?);
                    }
                }
                bail!("工作区中没有这个技能")
            }
            "websearch" => {
                let query = s(a, "query")?;
                if query.trim().is_empty() {
                    bail!("搜索关键词不能为空")
                }
                let mut url = reqwest::Url::parse("https://html.duckduckgo.com/html/")?;
                url.query_pairs_mut().append_pair("q", query);
                let response = reqwest::Client::builder()
                    .timeout(Duration::from_secs(20))
                    .build()?
                    .get(url)
                    .header("User-Agent", "Guanlan/0.1 (desktop coding assistant)")
                    .send()
                    .await?
                    .error_for_status()?;
                let text = fetch_bounded(response).await?;
                search_results(
                    &text,
                    a["numResults"].as_u64().unwrap_or(8).clamp(1, 20) as usize,
                )
            }
            "webfetch" => {
                let url = reqwest::Url::parse(s(a, "url")?)?;
                if !matches!(url.scheme(), "http" | "https") {
                    bail!("仅支持 HTTP(S)")
                };
                let response = reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()?
                    .get(url)
                    .send()
                    .await?
                    .error_for_status()?;
                fetch_bounded(response).await
            }
            _ => bail!("未知工具：{name}"),
        }
    }
    fn search(&self, name: &str, a: &Value) -> Result<String> {
        let path = safe_path(&self.root, a["path"].as_str().unwrap_or("."))?;
        let depth = if name == "list_dir" && !a["recursive"].as_bool().unwrap_or(false) {
            1
        } else {
            a["maxdepth"].as_u64().unwrap_or(30).min(50) as usize
        };
        let filter = if name == "glob" {
            Some(globset::Glob::new(s(a, "pattern")?)?.compile_matcher())
        } else {
            a["include"]
                .as_str()
                .map(|s| globset::Glob::new(s).map(|g| g.compile_matcher()))
                .transpose()?
        };
        let re = if name == "grep" {
            Some(
                regex::RegexBuilder::new(s(a, "pattern")?)
                    .case_insensitive(true)
                    .build()?,
            )
        } else {
            None
        };
        let mut out = vec![];
        for entry in WalkDir::new(&path)
            .max_depth(depth)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !ignored(e.file_name().to_string_lossy().as_ref()))
        {
            let e = entry?;
            if e.file_type().is_symlink() {
                continue;
            }
            let relative = e
                .path()
                .strip_prefix(&self.root)?
                .to_string_lossy()
                .replace('\\', "/");
            if let Some(g) = &filter {
                if !g.is_match(&relative) && !g.is_match(e.file_name()) {
                    continue;
                }
            }
            if let Some(re) = &re {
                if !e.file_type().is_file() || e.metadata()?.len() > 2 * 1024 * 1024 {
                    continue;
                }
                if let Ok(t) = fs::read_to_string(e.path()) {
                    for (i, l) in t.lines().enumerate() {
                        if re.is_match(l) {
                            out.push(format!(
                                "{relative}:{}: {}",
                                i + 1,
                                l.chars().take(300).collect::<String>()
                            ));
                            if out.len() >= 100 {
                                break;
                            }
                        }
                    }
                }
            } else if e.depth() > 0 {
                out.push(format!(
                    "{relative}{}",
                    if e.file_type().is_dir() { "/" } else { "" }
                ));
            }
            if out.len() >= 200 {
                out.push("…已达到显示上限".into());
                break;
            }
        }
        Ok(if out.is_empty() {
            "没有匹配结果".into()
        } else {
            out.join("\n")
        })
    }
    fn patch(&mut self, patch: &str) -> Result<String> {
        let lines: Vec<_> = patch.lines().collect();
        if lines.first() != Some(&"*** Begin Patch") || lines.last() != Some(&"*** End Patch") {
            bail!("补丁需要 Begin/End Patch")
        }
        let mut i = 1;
        let mut plan: Vec<(PathBuf, Option<String>)> = vec![];
        while i < lines.len() - 1 {
            let header = lines[i];
            i += 1;
            let (kind, name) = if let Some(n) = header.strip_prefix("*** Add File: ") {
                ("add", n)
            } else if let Some(n) = header.strip_prefix("*** Update File: ") {
                ("update", n)
            } else if let Some(n) = header.strip_prefix("*** Delete File: ") {
                ("delete", n)
            } else {
                bail!("无效补丁头：{header}")
            };
            let p = safe_path(&self.root, name)?;
            if plan.iter().any(|(q, _)| q == &p) {
                bail!("同一补丁不能重复修改一个文件")
            }
            let start = i;
            while i < lines.len() - 1
                && (!lines[i].starts_with("*** ") || lines[i] == "*** End of File")
            {
                i += 1
            }
            let body = &lines[start..i];
            let content = match kind {
                "add" => {
                    if p.exists() {
                        bail!("Add File 目标已存在")
                    };
                    let mut t = String::new();
                    for l in body {
                        t.push_str(l.strip_prefix('+').context("新增文件行需以 + 开始")?);
                        t.push('\n');
                    }
                    Some(t)
                }
                "delete" => {
                    if !p.is_file() {
                        bail!("删除目标不是文件")
                    };
                    if !body.is_empty() {
                        bail!("Delete File 不接受补丁内容")
                    };
                    None
                }
                _ => {
                    let old = fs::read_to_string(&p)?;
                    Some(update_patch(&old, body)?)
                }
            };
            plan.push((p, content));
        }
        // Validate all hunks before applying any file modification.
        let mut diffs = vec![];
        for (p, content) in plan {
            if let Some(c) = content {
                diffs.push(self.write(&p, &c)?)
            } else {
                let before = fs::read_to_string(&p)?;
                self.snapshot(&p)?;
                fs::remove_file(&p)?;
                let d = similar::TextDiff::from_lines(before.as_str(), "")
                    .unified_diff()
                    .header(&p.to_string_lossy(), "/dev/null")
                    .to_string();
                self.changes
                    .push(json!({"path":p.strip_prefix(&self.root)?.to_string_lossy(),"diff":d}));
                diffs.push(d);
            }
        }
        Ok(diffs.join("\n"))
    }
}
async fn fetch_bounded(response: reqwest::Response) -> Result<String> {
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut data = vec![];
    while let Some(chunk) = stream.next().await {
        data.extend_from_slice(&chunk?);
        if data.len() > 2 * 1024 * 1024 {
            bail!("网页超过 2 MB 上限")
        }
    }
    Ok(String::from_utf8_lossy(&data).into_owned())
}
fn search_results(html: &str, limit: usize) -> Result<String> {
    if html.contains("anomaly.js") || html.contains("challenge-form") {
        bail!("搜索服务要求人工验证，当前无法自动搜索")
    }
    let anchors = regex::Regex::new(r#"(?is)<a\b([^>]*)>(.*?)</a>"#)?;
    let href = regex::Regex::new(r#"(?i)href\s*=\s*"([^"]+)""#)?;
    let tags = regex::Regex::new("<[^>]+>")?;
    let mut result = vec![];
    for capture in anchors.captures_iter(html) {
        if !capture[1].contains("result__a") {
            continue;
        }
        if let Some(h) = href.captures(&capture[1]) {
            let raw = html_escape::decode_html_entities(&h[1]);
            let u = reqwest::Url::parse("https://duckduckgo.com")?.join(&raw)?;
            let url = u
                .query_pairs()
                .find(|(k, _)| k == "uddg")
                .map(|(_, v)| v.into_owned())
                .unwrap_or(u.to_string());
            let parsed = reqwest::Url::parse(&url)?;
            if !matches!(parsed.scheme(), "http" | "https") {
                continue;
            }
            let title =
                html_escape::decode_html_entities(&tags.replace_all(&capture[2], "")).into_owned();
            result.push(format!("{}. {}\n{}", result.len() + 1, title, parsed));
            if result.len() >= limit {
                break;
            }
        }
    }
    if result.is_empty() {
        bail!("搜索服务没有返回可解析的结果；可能为空或页面格式变化")
    };
    Ok(result.join("\n\n"))
}
pub fn s<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .with_context(|| format!("缺少字符串参数 {key}"))
}
fn ignored(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".guanlan" | "node_modules" | "target" | ".venv" | "__pycache__" | "dist"
    )
}
fn update_patch(old: &str, body: &[&str]) -> Result<String> {
    let mut text: Vec<String> = old.lines().map(str::to_owned).collect();
    let mut cursor = 0;
    let mut i = 0;
    while i < body.len() {
        if body[i].starts_with("@@") {
            i += 1;
            continue;
        }
        if body[i] == "*** End of File" {
            i += 1;
            continue;
        }
        let mut before = vec![];
        let mut after = vec![];
        while i < body.len() && !body[i].starts_with("@@") && body[i] != "*** End of File" {
            let l = body[i];
            i += 1;
            if let Some(t) = l.strip_prefix(' ') {
                before.push(t.to_owned());
                after.push(t.to_owned());
            } else if let Some(t) = l.strip_prefix('-') {
                before.push(t.to_owned());
            } else if let Some(t) = l.strip_prefix('+') {
                after.push(t.to_owned());
            } else {
                bail!("无效补丁行：{l}")
            }
        }
        if before.is_empty() {
            bail!("更新补丁需要至少一行定位上下文")
        }
        let positions: Vec<_> = (cursor..=text.len().saturating_sub(before.len()))
            .filter(|p| text.get(*p..*p + before.len()) == Some(before.as_slice()))
            .collect();
        if positions.len() != 1 {
            bail!("补丁上下文未匹配或匹配多处")
        };
        let p = positions[0];
        let n = after.len();
        text.splice(p..p + before.len(), after);
        cursor = p + n;
    }
    let mut result = text.join("\n");
    if old.ends_with('\n') {
        result.push('\n')
    }
    Ok(result)
}
async fn shell(
    command: &str,
    root: &Path,
    seconds: u64,
    cancel: &CancellationToken,
) -> Result<String> {
    let mut cmd = if cfg!(windows) {
        let program = std::env::var_os("ProgramFiles")
            .map(|p| Path::new(&p).join("PowerShell/7/pwsh.exe"))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| "powershell.exe".into());
        let mut c = tokio::process::Command::new(program);
        let script = format!(
            "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); \
             [Console]::InputEncoding = [Text.UTF8Encoding]::new($false); \
             $OutputEncoding = [Console]::OutputEncoding; \
             $PSDefaultParameterValues['*:Encoding'] = 'utf8'; \
             $global:LASTEXITCODE = 0; \
             if ($PSVersionTable.PSVersion -ge [version]'7.3') {{ \
               $PSNativeCommandUseErrorActionPreference = $true; $ErrorActionPreference = 'Stop' \
             }}; try {{ & {{\n{command}\n}}; \
             $ok = $?; if ($LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}; \
             if (!$ok) {{ exit 1 }} \
             }} catch {{ [Console]::Error.WriteLine(($_ | Out-String)); exit 1 }}"
        );
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        // GUI processes may inherit PATH from before Rust was installed.
        if let Some(home) = std::env::var_os("USERPROFILE") {
            let cargo = Path::new(&home).join(".cargo/bin");
            if cargo.is_dir() {
                let mut paths = vec![cargo];
                if let Some(path) = std::env::var_os("PATH") {
                    paths.extend(std::env::split_paths(&path));
                }
                if let Ok(path) = std::env::join_paths(paths) {
                    c.env("PATH", path);
                }
            }
        }
        c
    } else {
        let mut c = tokio::process::Command::new("bash");
        c.args(["-c", command]);
        c
    };
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let mut child = cmd
        .current_dir(root)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let pid = child.id();
    let out = tokio::spawn(capped_output(
        child.stdout.take().context("stdout unavailable")?,
    ));
    let err = tokio::spawn(capped_output(
        child.stderr.take().context("stderr unavailable")?,
    ));
    let status = tokio::select! {result=child.wait()=>Some(result?),_=cancel.cancelled()=>None,_=tokio::time::sleep(Duration::from_secs(seconds))=>None};
    if let Some(status) = status {
        let stdout = tokio::time::timeout(Duration::from_secs(2), out).await??;
        let stderr = tokio::time::timeout(Duration::from_secs(2), err).await??;
        let output = format!(
            "exit: {}\nstdout:\n{}\nstderr:\n{}",
            status,
            String::from_utf8_lossy(&stdout?),
            String::from_utf8_lossy(&stderr?)
        );
        if status.success() {
            Ok(output)
        } else {
            bail!("{output}")
        }
    } else {
        #[cfg(windows)]
        if let Some(id) = pid {
            let mut kill = tokio::process::Command::new("taskkill");
            kill.creation_flags(0x08000000);
            let _ = kill
                .args(["/PID", &id.to_string(), "/T", "/F"])
                .output()
                .await;
        }
        #[cfg(not(windows))]
        let _ = pid;
        let _ = child.kill().await;
        out.abort();
        err.abort();
        bail!("命令已取消或超时；命令造成的文件变更不能通过文件撤销恢复")
    }
}
async fn capped_output<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut out = vec![];
    let mut chunk = [0u8; 4096];
    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        let keep = n.min(30_000usize.saturating_sub(out.len()));
        out.extend_from_slice(&chunk[..keep]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[tokio::test]
    async fn shell_reports_failure_and_preserves_utf8() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("sample.txt"), "中文牙体测试").unwrap();
        let token = CancellationToken::new();
        let output = shell(
            "Get-Content sample.txt; Write-Output '中文输出'",
            &dir,
            15,
            &token,
        )
        .await
        .unwrap();
        assert!(output.contains("中文牙体测试"));
        assert!(output.contains("中文输出"));
        let error = shell("Write-Output '失败详情'; exit 3", &dir, 15, &token)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("失败详情"));
        assert!(error.contains('3'));
        let error = shell("cmd.exe /c exit 7; Write-Output '假成功'", &dir, 15, &token)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains('7'));
        let output = shell(
            "cmd.exe /c 'echo diagnostic 1>&2' 2>&1; Write-Output '完成'",
            &dir,
            15,
            &token,
        )
        .await
        .unwrap();
        assert!(output.contains("diagnostic"));
        assert!(output.contains("完成"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn search_redirect_and_entities() {
        let html = r#"<a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fdocs" class="result__a"><b>Rust</b> &amp; tools</a>"#;
        let result = search_results(html, 8).unwrap();
        assert!(result.contains("Rust & tools"));
        assert!(result.contains("https://example.com/docs"));
        assert!(search_results("<form id=\"challenge-form\">", 8).is_err());
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn cancellation_terminates_powershell_tree() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("child.pid");
        let command=format!("$p = Start-Process powershell.exe -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 60' -WindowStyle Hidden -PassThru; [IO.File]::WriteAllText('{}',[string]$p.Id); Start-Sleep -Seconds 60",path.to_string_lossy().replace('\'',"''"));
        let token = CancellationToken::new();
        let c = token.clone();
        let cwd = dir.clone();
        let task = tokio::spawn(async move { shell(&command, &cwd, 30, &c).await });
        for _ in 0..200 {
            if path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        token.cancel();
        assert!(tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        let pid = fs::read_to_string(&path).expect("child process started");
        let mut check = tokio::process::Command::new("powershell.exe");
        check.creation_flags(0x08000000);
        let status=check.args(["-NoProfile","-Command",&format!("if (Get-Process -Id {} -ErrorAction SilentlyContinue) {{exit 1}} else {{exit 0}}",pid.trim())]).status().await.unwrap();
        assert!(
            status.success(),
            "child process must not survive cancellation"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn patch_ambiguous_context_rejected() {
        assert!(update_patch("a\na\n", &["@@", "-a", "+b"]).is_err());
        assert_eq!(
            update_patch("a\nb\nc\n", &["@@", " a", "-b", "+B", " c"]).unwrap(),
            "a\nB\nc\n"
        );
    }
    #[tokio::test]
    async fn write_edit_undo_exact_bytes() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).unwrap();
        let root = fs::canonicalize(&dir).unwrap();
        fs::write(root.join("a.txt"), b"original\r\n").unwrap();
        let mut t = Tools::new(root.clone());
        let c = CancellationToken::new();
        t.execute("write", &json!({"path":"a.txt","content":"changed"}), &c)
            .await
            .unwrap();
        t.execute("write", &json!({"path":"new.txt","content":"new"}), &c)
            .await
            .unwrap();
        let mut session = crate::Session::default();
        session.undo.push(t.record());
        crate::undo(&root, &mut session).unwrap();
        assert_eq!(fs::read(root.join("a.txt")).unwrap(), b"original\r\n");
        assert!(!root.join("new.txt").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
