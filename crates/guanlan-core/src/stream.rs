use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Sse {
    buffer: Vec<u8>,
}
impl Sse {
    /// Buffer bytes, not decoded fragments: a UTF-8 character may cross network packets.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > 8 * 1024 * 1024 {
            bail!("流式响应单帧过大")
        }
        let mut result = vec![];
        while let Some(pos) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.buffer.drain(..=pos).collect();
            let line = std::str::from_utf8(&line)?.trim_end_matches(['\r', '\n']);
            if let Some(data) = line.strip_prefix("data:") {
                result.push(data.trim_start().to_owned());
            }
        }
        Ok(result)
    }
    pub fn finish(&mut self) -> Result<Vec<String>> {
        if self.buffer.is_empty() {
            Ok(vec![])
        } else {
            self.push(b"\n")
        }
    }
}
#[derive(Default)]
pub struct Assembly {
    pub content: String,
    pub reasoning: String,
    calls: BTreeMap<usize, Value>,
    pub finish: Option<String>,
}
impl Assembly {
    pub fn accept(&mut self, data: &str) -> Result<Vec<(&'static str, String)>> {
        if data == "[DONE]" {
            return Ok(vec![]);
        }
        let v: Value = serde_json::from_str(data).context("无法解析模型流式响应")?;
        if !v["error"].is_null() {
            bail!("模型服务错误：{}", v["error"])
        }
        let c = &v["choices"][0];
        let d = &c["delta"];
        let mut events = vec![];
        if let Some(s) = d["content"].as_str() {
            self.content.push_str(s);
            events.push(("content", s.into()));
        }
        if let Some(s) = d["reasoning_content"].as_str().or(d["reasoning"].as_str()) {
            self.reasoning.push_str(s);
            events.push(("reasoning", s.into()));
        }
        if let Some(cs) = d["tool_calls"].as_array() {
            for t in cs {
                let i = t["index"].as_u64().unwrap_or(0) as usize;
                let call = self.calls.entry(i).or_insert_with(
                    || json!({"id":"","type":"function","function":{"name":"","arguments":""}}),
                );
                if let Some(s) = t["id"].as_str() {
                    call["id"] = json!(s);
                }
                for k in ["name", "arguments"] {
                    if let Some(s) = t["function"][k].as_str() {
                        let current = call["function"][k].as_str().unwrap_or("");
                        call["function"][k] = json!(format!("{current}{s}"));
                    }
                }
            }
        }
        if let Some(f) = c["finish_reason"].as_str() {
            self.finish = Some(f.into());
        }
        Ok(events)
    }
    pub fn message(&self) -> Value {
        let mut v = json!({"role":"assistant","content":self.content});
        if !self.reasoning.is_empty() {
            v["reasoning_content"] = json!(self.reasoning);
        }
        if !self.calls.is_empty() {
            v["tool_calls"] = json!(self.calls.values().collect::<Vec<_>>());
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_utf8_and_crlf() {
        let bytes = "data: 中文\r\n\r\n".as_bytes();
        let mut p = Sse::default();
        let mut out = vec![];
        for b in bytes {
            out.extend(p.push(&[*b]).unwrap());
        }
        assert_eq!(out, vec!["中文"]);
    }
    #[test]
    fn fragmented_parallel_calls() {
        let mut a = Assembly::default();
        a.accept(r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"b","function":{"name":"write","arguments":"{\"path\":"}},{"index":0,"id":"a","function":{"name":"read","arguments":"{}"}}]}}]}"#).unwrap();
        a.accept(r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"\"x\"}"}}]},"finish_reason":"tool_calls"}]}"#).unwrap();
        let m = a.message();
        assert_eq!(m["tool_calls"][0]["id"], "a");
        assert_eq!(
            m["tool_calls"][1]["function"]["arguments"],
            r#"{"path":"x"}"#
        );
    }
}
