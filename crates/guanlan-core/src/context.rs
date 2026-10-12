use serde_json::{json, Value};

/// Preserve complete user/tool transactions and a bounded extract of the earlier goals.
/// This is deterministic extraction, not an LLM-generated summary.
pub fn prepare(history: &[Value], budget: usize) -> (Vec<Value>, bool) {
    let mut messages = history.to_vec();
    for m in &mut messages {
        if let Some(o) = m.as_object_mut() {
            o.remove("reasoning_content");
        }
    }
    if serde_json::to_vec(&messages).map_or(0, |s| s.len()) <= budget {
        return (messages, false);
    }
    for m in &mut messages {
        if m["role"] == "tool" {
            if let Some(content) = m["content"].as_str() {
                if content.len() > 8_000 {
                    m["content"] = json!(format!(
                        "{}\n[工具输出已截短，完整结果保存在本地历史中]",
                        content.chars().take(2000).collect::<String>()
                    ));
                }
            }
        }
    }
    let mut start = messages
        .iter()
        .rposition(|m| m["role"] == "user")
        .unwrap_or(0);
    for (i, m) in messages.iter().enumerate().rev() {
        if m["role"] == "user"
            && serde_json::to_vec(&messages[i..]).map_or(usize::MAX, |s| s.len())
                < budget.saturating_sub(6000)
        {
            start = i;
        }
    }
    if start == 0 {
        return (messages, true);
    }
    let older = &messages[..start];
    let mut excerpts = vec![];
    if let Some(first) = older.iter().find(|m| m["role"] == "user") {
        excerpts.push(format!(
            "最初目标：{}",
            first["content"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(600)
                .collect::<String>()
        ));
    }
    for m in older
        .iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let role = m["role"].as_str().unwrap_or("");
        if matches!(role, "user" | "assistant" | "tool") {
            let text = m["content"].as_str().unwrap_or("");
            if !text.is_empty() {
                excerpts.push(format!(
                    "{role}: {}",
                    text.chars().take(220).collect::<String>()
                ));
            }
            if let Some(calls) = m["tool_calls"].as_array() {
                for c in calls {
                    excerpts.push(format!(
                        "工具调用：{} {}",
                        c["function"]["name"].as_str().unwrap_or(""),
                        c["function"]["arguments"]
                            .as_str()
                            .unwrap_or("")
                            .chars()
                            .take(180)
                            .collect::<String>()
                    ));
                }
            }
        }
    }
    let mut result = vec![
        json!({"role":"user","content":format!("[早期对话节选，可能不完整；以最近消息和实际文件为准]\n{}",excerpts.join("\n"))}),
        json!({"role":"assistant","content":"已接收早期对话节选。后续以当前需求与实际工具结果为准。"}),
    ];
    result.extend_from_slice(&messages[start..]);
    (result, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keep_tool_transaction_and_first_goal() {
        let h = vec![
            json!({"role":"user","content":format!("original {}","x".repeat(25_000))}),
            json!({"role":"assistant","content":"old answer"}),
            json!({"role":"user","content":"latest"}),
            json!({"role":"assistant","tool_calls":[{"id":"a"}]}),
            json!({"role":"tool","tool_call_id":"a","content":"ok"}),
        ];
        let (m, changed) = prepare(&h, 10_000);
        assert!(changed);
        assert!(m[0]["content"].as_str().unwrap().contains("original"));
        assert_eq!(m[2]["content"], "latest");
        assert_eq!(m[4]["tool_call_id"], "a");
    }
}
