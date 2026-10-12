import { invoke, Channel, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { marked } from "marked";
import DOMPurify from "dompurify";
import {
  createIcons,
  Plus,
  FolderOpen,
  Folder,
  File,
  SlidersHorizontal,
  Undo2,
  PanelRight,
  ArrowUp,
  Square,
  Activity,
  FileDiff,
  ListChecks,
  X,
  ScanText,
  Bug,
  Blocks,
  MessageSquare,
  Terminal,
  ShieldCheck,
  MessageCircle,
  CircleCheck,
  LoaderCircle,
  Circle,
} from "lucide";
import "./style.css";
import "./material.css";
import guanlanIcon from "./assets/guanlan-icon.png";
import { ModelPicker } from "./model-picker";
import { finishStreamMessage } from "./stream-status";

type Settings = {
  workspace: string;
  base_url: string;
  model: string;
  mode: string;
  auto_approve: boolean;
  max_steps: number;
  context_budget: number;
};
type Message = {
  role: string;
  content?: string;
  reasoning_content?: string;
  tool_call_id?: string;
  tool_calls?: Array<{
    id: string;
    function: { name: string; arguments: string };
  }>;
};
type Session = {
  id: string;
  title: string;
  messages: Message[];
  undo_count: number;
};
type Event = { type: string; [key: string]: any };
let settings: Settings = {
  workspace: "",
  base_url: "https://api.deepseek.com/v1",
  model: "",
  mode: "build",
  auto_approve: false,
  max_steps: 30,
  context_budget: 120000,
};
let session: Session = { id: "", title: "新对话", messages: [], undo_count: 0 };
let busy = false,
  hasKey = false,
  assistant: HTMLElement | null = null,
  content = "",
  reasoning = "",
  renderTimer = 0;
let streamPending = true;
const $ = <T extends HTMLElement = HTMLElement>(id: string) =>
  document.getElementById(id) as T;
const escape = (s: string) =>
  s.replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
const md = (s: string) =>
  DOMPurify.sanitize(marked.parse(s, { async: false }) as string, {
    FORBID_TAGS: ["img", "iframe", "form", "input", "button"],
    FORBID_ATTR: ["style"],
  });
const icon = (name: string) => `<i data-lucide="${name}"></i>`;
function refreshIcons() {
  createIcons({
    icons: {
      Plus,
      FolderOpen,
      Folder,
      File,
      SlidersHorizontal,
      Undo2,
      PanelRight,
      ArrowUp,
      Square,
      Activity,
      FileDiff,
      ListChecks,
      X,
      ScanText,
      Bug,
      Blocks,
      MessageSquare,
      Terminal,
      ShieldCheck,
      MessageCircle,
      CircleCheck,
      LoaderCircle,
      Circle,
    },
    attrs: { "stroke-width": 1.6 },
  });
}
document.querySelector("#app")!.innerHTML = `
<aside class="sidebar">
  <div class="brand"><div class="brand-mark"><img src="${guanlanIcon}" alt=""/></div><div><strong>观澜</strong><span>GUANLAN</span></div><span class="version">0.1.7</span></div>
  <button id="new" class="new-session">${icon("plus")}新建对话<span>Ctrl N</span></button>
  <div class="section-label">对话记录</div><nav id="sessions" aria-label="对话记录"><div class="sidebar-empty">暂无对话</div></nav>
  <div class="workspace-section"><div class="section-label">工作目录<button id="folder" class="icon-button" title="选择工作目录">${icon("folder-open")}</button></div><button id="workspace" title="选择工作目录">${icon("folder")}<span>选择一个文件夹</span></button><div id="tree"></div></div>
  <div class="sidebar-footer"><button id="settings">${icon("sliders-horizontal")}模型与设置</button><span id="connection" hidden></span></div>
</aside>
<main>
  <header class="topbar"><div><strong id="title">新对话</strong></div><div class="top-actions"><select id="mode" aria-label="工作模式"><option value="build">执行模式</option><option value="plan">规划模式</option></select><span class="separator"></span><button id="undo" title="撤销最近一轮文件工具修改">${icon("undo-2")}撤销修改</button><button id="toggle-inspector" class="icon-button" title="展开或收起执行面板">${icon("panel-right")}</button></div></header>
  <div id="conversation" class="conversation" aria-live="polite"></div>
  <div class="composer-area"><div id="request" hidden></div><div class="composer"><textarea id="prompt" rows="3" aria-label="消息输入"></textarea><div class="composer-bottom"><button id="model-label" type="button" title="选择模型">请选择模型</button><span id="run-status">Enter 发送 · Shift Enter 换行</span><button id="send" class="send" title="发送消息">${icon("arrow-up")}</button><button id="stop" class="stop" hidden>${icon("square")}停止</button></div></div></div>
<div class="quick-actions suggestions"><button data-prompt="查看当前项目，解释它的结构和运行方式。先不要修改文件。">${icon("scan-text")}了解项目</button><button data-prompt="帮我排查一个问题。请先询问具体症状，再检查相关代码。">${icon("bug")}排查问题</button><button data-prompt="帮我实现一个功能。请先询问需求，并给出实施计划。">${icon("blocks")}实现功能</button></div>
</main>
<aside id="inspector" class="inspector collapsed"><div class="inspector-head"><strong>工作台</strong><span id="activity-status">就绪</span></div><div class="tabs"><button data-tab="activity" class="active">执行</button><button data-tab="changes">修改 <span id="change-count">0</span></button><button data-tab="tasks">计划</button></div><div id="activity" class="inspector-body"><div class="panel-empty">${icon("activity")}<strong>暂无执行记录</strong><p>运行任务后查看工具调用与结果</p></div></div><div id="changes" class="inspector-body" hidden><div class="panel-empty">${icon("file-diff")}<p>文件工具的修改会显示差异。<br>命令执行的文件变更不在此列表中。</p></div></div><div id="tasks" class="inspector-body" hidden><div class="panel-empty">${icon("list-checks")}<p>助手创建计划后会显示在这里。</p></div></div></aside>
<dialog id="settings-dialog"><form id="settings-form"><div class="dialog-head"><div><h2>模型与工作环境</h2></div><button type="button" id="close-settings" class="icon-button" aria-label="关闭">${icon("x")}</button></div><p class="dialog-intro">连接一个兼容 OpenAI Chat Completions 的服务。</p><div class="appearance-field"><span>界面材质</span><select id="appearance" aria-label="界面材质"><option value="glass">液态玻璃</option><option value="solid">纯色</option></select></div><label>服务商<select id="provider"><option value="deepseek">DeepSeek</option><option value="openai">OpenAI</option><option value="qwen">通义千问</option><option value="siliconflow">硅基流动</option><option value="openrouter">OpenRouter</option><option value="ollama">Ollama（本地）</option><option value="custom">自定义</option></select></label><label>API 地址<input id="base-url" type="url" required placeholder="https://…/v1"/><small>填写 API 根地址，通常以 /v1 结尾。</small></label><label>模型名称<div class="input-row"><div class="model-picker"><input id="model" required autocomplete="off" role="combobox" aria-autocomplete="list" aria-controls="models-list" aria-expanded="false" placeholder="输入名称筛选模型"/><button id="model-toggle" type="button" class="model-toggle" aria-label="展开模型列表">▾</button><div id="models-list" class="model-options" role="listbox" aria-label="符合的模型" hidden></div></div><button id="discover" type="button">获取模型</button></div><small id="model-status" role="status">可手动输入，或获取模型后从候选列表选择。</small></label><label>API 密钥<input id="api-key" type="password" autocomplete="off" placeholder="留空保留已保存的密钥"/><small id="key-status">密钥保存在 Windows 凭据管理器中。</small></label><label class="check"><input id="clear-key" type="checkbox"/>删除这个服务的已保存密钥</label><label>工作目录<div class="input-row"><input id="workspace-path" required/><button id="pick" type="button">选择</button></div></label><div class="two-col"><label>最多执行步数<input id="max-steps" type="number" min="1" max="100" required/></label><label>上下文预算（字节）<input id="context-budget" type="number" min="20000" max="500000" required/></label></div><label class="check"><input id="auto-approve" type="checkbox"/>自动批准文件写入及命令执行</label><div id="settings-error" class="error" hidden></div><div class="dialog-footer"><button type="button" id="cancel-settings">取消</button><button type="submit" class="primary">保存设置</button></div></form></dialog>
<div id="toast" role="status" hidden></div>`;

const appearance = document.querySelector<HTMLSelectElement>("#appearance")!;
let material = matchMedia("(prefers-reduced-transparency: reduce)").matches ? "solid" : "glass";
try { const saved = localStorage.getItem("guanlan-material"); if (saved === "solid" || saved === "glass") material = saved; } catch {}
document.documentElement.dataset.material = material;
appearance.value = material;
appearance.onchange = () => {
  document.documentElement.dataset.material = appearance.value;
  try { localStorage.setItem("guanlan-material", appearance.value); } catch {}
};

function welcome() {
  document.querySelector("main")!.classList.add("is-empty");
  $("conversation").innerHTML =
    `<div class="welcome"><div class="welcome-symbol"><img src="${guanlanIcon}" alt=""/></div><h1>开始一个新任务</h1></div>`;
  document.querySelectorAll<HTMLButtonElement>("[data-prompt]").forEach(
    (b) =>
      (b.onclick = () => {
        $<HTMLTextAreaElement>("prompt").value = b.dataset.prompt!;
        $("prompt").focus();
      }),
  );
  refreshIcons();
}
function renderMessages() {
  document.querySelector("main")!.classList.remove("is-empty");
  const conv = $("conversation");
  conv.innerHTML = "";
  if (!session.messages.length) {
    welcome();
    return;
  }
  for (const m of session.messages) {
    if (m.role === "user") {
      const el = document.createElement("article");
      el.className = "message user";
      el.innerHTML = `<div class="message-label">你</div><div class="user-content">${escape(m.content || "")}</div>`;
      conv.append(el);
    } else if (m.role === "assistant") {
      const el = document.createElement("article");
      el.className = "message assistant";
      el.innerHTML = `<div class="message-label"><span class="mini-brand"><img src="${guanlanIcon}" alt=""/></span>观澜</div>${m.reasoning_content ? `<details class="reasoning"><summary>思考过程</summary><div>${escape(m.reasoning_content)}</div></details>` : ""}<div class="markdown">${md(m.content || "")}</div>${m.tool_calls?.length ? `<div class="inline-tools">${m.tool_calls.map((t) => `<span>${escape(t.function.name)}</span>`).join("")}</div>` : ""}`;
      conv.append(el);
    } else if (m.role === "tool") {
      const el = document.createElement("details");
      el.className = "tool-result";
      el.innerHTML = `<summary>${icon("terminal")}工具结果 <span>${m.content?.startsWith("ERROR") ? "未完成" : "已返回"}</span></summary><pre>${escape(m.content || "")}</pre>`;
      conv.append(el);
    }
  }
  refreshIcons();
  conv.scrollTop = conv.scrollHeight;
}
function toast(text: string) {
  $("toast").textContent = text;
  $("toast").hidden = false;
  window.setTimeout(() => ($("toast").hidden = true), 5500);
}
function setBusy(value: boolean) {
  busy = value;
  ["new", "settings", "folder", "workspace", "undo", "mode"].forEach(
    (id) => ($<HTMLButtonElement>(id).disabled = value),
  );
  document
    .querySelectorAll<HTMLButtonElement>(".session-item")
    .forEach((b) => (b.disabled = value));
  $("send").hidden = value;
  $("stop").hidden = !value;
  $<HTMLTextAreaElement>("prompt").disabled = value;
  $("activity-status").textContent = value ? "进行中" : "就绪";
  $("request").hidden = true;
  $("request").innerHTML = "";
}
async function bootstrap() {
  if (!isTauri()) {
    welcome();
    $("connection").textContent = "界面预览";
    toast("这是界面预览。启动桌面应用后可连接模型和读写文件。");
    return;
  }
  const data = await invoke<any>("bootstrap");
  settings = data.settings;
  if (session.id && session.id !== data.session.id) resetWorkbench();
  session = data.session;
  $<HTMLButtonElement>("undo").disabled = busy || session.undo_count === 0;
  hasKey = data.has_key;
  $("title").textContent = session.title;
  $<HTMLSelectElement>("mode").value = settings.mode;
  $("model-label").textContent = settings.model || "请选择模型";
  const label = $("workspace").querySelector("span")!;
  label.textContent = settings.workspace
    ? settings.workspace.split(/[\\/]/).pop()!
    : "选择一个文件夹";
  $("workspace").title = settings.workspace || "选择工作目录";
  $("sessions").innerHTML = data.sessions.length
    ? data.sessions
        .map(
          (s: any) =>
            `<button class="session-item ${s.id === session.id ? "selected" : ""}" data-id="${escape(s.id)}">${icon("message-square")}<span>${escape(s.title)}</span></button>`,
        )
        .join("")
    : '<div class="sidebar-empty">还没有保存的对话</div>';
  document.querySelectorAll<HTMLButtonElement>(".session-item").forEach(
    (b) =>
      (b.onclick = async () => {
        if (busy) return;
        try {
          session = await invoke<Session>("load_session", { id: b.dataset.id });
          resetWorkbench();
          renderMessages();
          $("title").textContent = session.title;
          await refreshSessionList();
        } catch (e) {
          toast(String(e));
        }
      }),
  );
  try {
    const files = await invoke<string[]>("workspace_files");
    $("tree").innerHTML = files
      .slice(0, 35)
      .map(
        (f) =>
          `<div title="${escape(f)}">${icon(f.endsWith("/") ? "folder" : "file")}<span>${escape(f)}</span></div>`,
      )
      .join("");
  } catch {
    $("tree").innerHTML = "";
  }
  renderMessages();
  refreshIcons();
}
async function refreshSessionList() {
  await bootstrap();
}
function renderStream() {
  if (!assistant) return;
  const c = $("conversation");
  const follow = c.scrollHeight - c.scrollTop - c.clientHeight < 130;
  assistant.innerHTML = `<div class="message-label"><span class="mini-brand"><img src="${guanlanIcon}" alt=""/></span>观澜 <span class="stream-dot"></span></div>${reasoning ? `<details class="reasoning"><summary>思考过程</summary><div>${escape(reasoning)}</div></details>` : ""}<div class="markdown">${content ? md(content) : '<span class="waiting">正在思考…</span>'}</div>`;
  if (!streamPending) finishStreamMessage(assistant);
  if (follow) c.scrollTop = c.scrollHeight;
}
function scheduleStream() {
  if (!renderTimer) {
    renderTimer = window.setTimeout(() => {
      renderTimer = 0;
      renderStream();
    }, 35);
  }
}
function resetWorkbench() {
  $("activity").innerHTML =
    '<div class="panel-empty">当前对话的工具执行将显示在这里。<br>历史结果可在对话中展开查看。</div>';
  $("changes").innerHTML =
    '<div class="panel-empty">当前运行的文件差异将显示在这里。</div>';
  $("tasks").innerHTML = "";
  $("change-count").textContent = "0";
}
function event(e: Event) {
  switch (e.type) {
    case "content":
      content += e.text;
      scheduleStream();
      break;
    case "reasoning":
      reasoning += e.text;
      scheduleStream();
      break;
    case "step":
      if (e.step > 1 && assistant && (content || reasoning)) {
        renderStream();
        finishStreamMessage(assistant);
        assistant = document.createElement("article");
        assistant.className = "message assistant";
        $("conversation").append(assistant);
        content = "";
        reasoning = "";
        renderStream();
      }
      streamPending = true;
      renderStream();
      $("run-status").textContent = `执行第 ${e.step} 步`;
      break;
    case "notice":
      toast(e.text);
      break;
    case "approval":
    case "question":
      request(e);
      break;
    case "tool_start": {
      streamPending = false;
      renderStream();
      if ($("activity").querySelector(".panel-empty"))
        $("activity").innerHTML = "";
      const el = document.createElement("details");
      el.className = "activity-card";
      el.dataset.id = e.id;
      el.innerHTML = `<summary><span class="tool-dot"></span><strong>${escape(e.name)}</strong><span class="tool-state">执行中</span></summary><pre>${escape(JSON.stringify(e.args, null, 2))}</pre>`;
      $("activity").append(el);
      break;
    }
    case "tool_finish": {
      const el = Array.from(
        document.querySelectorAll<HTMLElement>(".activity-card"),
      ).find((x) => x.dataset.id === e.id);
      if (el) {
        el.classList.add(e.ok ? "success" : "failed");
        el.querySelector(".tool-state")!.textContent = e.ok ? "完成" : "未完成";
        const pre = document.createElement("pre");
        pre.textContent = e.output;
        el.append(pre);
      }
      break;
    }
    case "change": {
      if ($("changes").querySelector(".panel-empty"))
        $("changes").innerHTML = "";
      const el = document.createElement("details");
      el.className = "change-card";
      el.innerHTML = `<summary>${icon("file-diff")}${escape(e.change.path)}</summary><pre>${escape(e.change.diff)}</pre>`;
      $("changes").prepend(el);
      $("change-count").textContent = String($("changes").children.length);
      refreshIcons();
      break;
    }
    case "todos":
      $("tasks").innerHTML = e.todos
        .map(
          (t: any) =>
            `<div class="todo ${t.status === "completed" ? "completed" : ""}">${icon(t.status === "completed" ? "circle-check" : t.status === "in_progress" ? "loader-circle" : "circle")}<span>${escape(t.content)}</span></div>`,
        )
        .join("");
      refreshIcons();
      break;
    case "done":
      clearTimeout(renderTimer);
      renderTimer = 0;
      assistant = null;
      setBusy(false);
      session = e.session;
      renderMessages();
      $("run-status").textContent = "Enter 发送 · Shift Enter 换行";
      if (e.error) toast(e.error);
      void bootstrap().catch((e) => toast(String(e)));
      break;
  }
}
function request(e: Event) {
  streamPending = false;
  renderStream();
  const el = $("request");
  el.hidden = false;
  el.innerHTML =
    e.type === "approval"
      ? `<div class="request-title">${icon("shield-check")}需要确认：${escape(e.name)}</div><pre>${escape(JSON.stringify(e.args, null, 2))}</pre><div class="request-actions"><span>确认后将执行此操作</span><button id="reject">拒绝</button><button id="approve" class="primary">批准一次</button></div>`
      : `<div class="request-title">${icon("message-circle")}观澜需要你的补充</div><p>${escape(e.text)}</p><div class="input-row"><input id="answer" placeholder="输入回复"/><button id="answer-send" class="primary">回复</button></div>`;
  const reply = async (answer: string) => {
    try {
      await invoke("respond", { id: e.id, answer });
      el.hidden = true;
      el.innerHTML = "";
    } catch (err) {
      toast(String(err));
    }
  };
  if (e.type === "approval") {
    $("approve").onclick = () => void reply("approve");
    $("reject").onclick = () => void reply("reject");
  } else {
    $("answer-send").onclick = () =>
      void reply($<HTMLInputElement>("answer").value);
    $<HTMLInputElement>("answer").onkeydown = (k) => {
      if (k.key === "Enter") void reply($<HTMLInputElement>("answer").value);
    };
    $("answer").focus();
  }
  refreshIcons();
}
async function send() {
  if (busy) return;
  const p = $<HTMLTextAreaElement>("prompt").value.trim();
  if (!p) return;
  if (!isTauri()) {
    toast("请通过 npm run desktop 启动桌面应用。");
    return;
  }
  if (!settings.workspace || !settings.model.trim()) {
    showSettings();
    toast("先选择工作目录并设置模型。");
    return;
  }
  $<HTMLTextAreaElement>("prompt").value = "";
  renderMessages();
  const conv = $("conversation");
  conv.querySelector(".welcome")?.remove();
  document.querySelector("main")!.classList.remove("is-empty");
  $("inspector").classList.remove("collapsed");
  const u = document.createElement("article");
  u.className = "message user";
  u.innerHTML = `<div class="message-label">你</div><div class="user-content">${escape(p)}</div>`;
  conv.append(u);
  assistant = document.createElement("article");
  assistant.className = "message assistant";
  conv.append(assistant);
  content = "";
  reasoning = "";
  streamPending = true;
  setBusy(true);
  renderStream();
  conv.scrollTop = conv.scrollHeight;
  const channel = new Channel<Event>();
  channel.onmessage = event;
  try {
    await invoke("start_run", { prompt: p, onEvent: channel });
  } catch (e) {
    setBusy(false);
    assistant = null;
    renderMessages();
    $<HTMLTextAreaElement>("prompt").value = p;
    toast(String(e));
  }
}
const presets: Record<string, [string, string]> = {
  deepseek: ["https://api.deepseek.com/v1", ""],
  openai: ["https://api.openai.com/v1", ""],
  qwen: ["https://dashscope.aliyuncs.com/compatible-mode/v1", ""],
  siliconflow: ["https://api.siliconflow.cn/v1", ""],
  openrouter: ["https://openrouter.ai/api/v1", ""],
  ollama: ["http://localhost:11434/v1", ""],
  custom: ["", ""],
};
const modelPicker = new ModelPicker(
  $<HTMLInputElement>("model"),
  $("model-toggle"),
  $("models-list"),
  $("model-status"),
);
$("base-url").addEventListener("input", () =>
  modelPicker.setEndpoint($<HTMLInputElement>("base-url").value),
);
$("api-key").addEventListener("input", () => modelPicker.invalidate());
$("clear-key").addEventListener("change", () => modelPicker.invalidate());
function showSettings() {
  if (busy) return;
  $<HTMLInputElement>("base-url").value = settings.base_url;
  $<HTMLInputElement>("model").value = settings.model;
  $<HTMLInputElement>("workspace-path").value = settings.workspace;
  $<HTMLInputElement>("max-steps").value = String(settings.max_steps);
  $<HTMLInputElement>("context-budget").value = String(settings.context_budget);
  $<HTMLInputElement>("auto-approve").checked = settings.auto_approve;
  $<HTMLInputElement>("api-key").value = "";
  $<HTMLInputElement>("clear-key").checked = false;
  $("key-status").textContent = hasKey
    ? "这个服务已保存密钥；留空即可保留。"
    : "密钥保存在 Windows 凭据管理器中。";
  $<HTMLSelectElement>("provider").value =
    Object.entries(presets).find(([, v]) => v[0] === settings.base_url)?.[0] ||
    "custom";
  modelPicker.setEndpoint(settings.base_url);
  modelPicker.close();
  $("settings-error").hidden = true;
  $<HTMLDialogElement>("settings-dialog").showModal();
}
async function pick() {
  if (!isTauri()) {
    toast("文件夹选择器仅在桌面程序中可用。");
    return;
  }
  const path = await open({
    directory: true,
    multiple: false,
    title: "选择观澜的工作目录",
  });
  if (path) $<HTMLInputElement>("workspace-path").value = String(path);
}
$("settings").onclick = showSettings;
$("folder").onclick = showSettings;
$("workspace").onclick = showSettings;
$("close-settings").onclick = $("cancel-settings").onclick = () =>
  $<HTMLDialogElement>("settings-dialog").close();
$("pick").onclick = () => void pick().catch((e) => toast(String(e)));
$<HTMLSelectElement>("provider").onchange = () => {
  const pair = presets[$<HTMLSelectElement>("provider").value];
  if (pair[0]) $<HTMLInputElement>("base-url").value = pair[0];
  $<HTMLInputElement>("model").value = pair[1];
  modelPicker.setEndpoint($<HTMLInputElement>("base-url").value);
  modelPicker.close();
};
$("settings-form").onsubmit = async (e) => {
  e.preventDefault();
  const next = {
    ...settings,
    workspace: $<HTMLInputElement>("workspace-path").value.trim(),
    base_url: $<HTMLInputElement>("base-url").value.trim(),
    model: $<HTMLInputElement>("model").value.trim(),
    max_steps: Number($<HTMLInputElement>("max-steps").value),
    context_budget: Number($<HTMLInputElement>("context-budget").value),
    auto_approve: $<HTMLInputElement>("auto-approve").checked,
  };
  const input = $<HTMLInputElement>("api-key").value;
  try {
    await invoke("save_settings", {
      settings: next,
      apiKey: $<HTMLInputElement>("clear-key").checked ? "" : input || null,
    });
    $<HTMLInputElement>("api-key").value = "";
    $<HTMLDialogElement>("settings-dialog").close();
    await bootstrap();
    toast("设置已保存");
  } catch (err) {
    $("settings-error").hidden = false;
    $("settings-error").textContent = String(err);
  }
};
$("discover").onclick = async () => {
  const b = $<HTMLButtonElement>("discover");
  b.disabled = true;
  const ticket = modelPicker.begin($<HTMLInputElement>("base-url").value);
  try {
    const models = await invoke<string[]>("discover_models", {
      baseUrl: ticket.endpoint,
      apiKey: $<HTMLInputElement>("api-key").value || null,
    });
    modelPicker.accept(ticket, models);
  } catch (e) {
    modelPicker.fail(ticket, String(e));
  } finally {
    b.disabled = false;
  }
};
$("send").onclick = () => void send();
$<HTMLTextAreaElement>("prompt").onkeydown = (e) => {
  if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
    e.preventDefault();
    void send();
  }
};
$("stop").onclick = () =>
  void invoke("stop_run").catch((e) => toast(String(e)));
$("new").onclick = async () => {
  try {
    session = await invoke<Session>("new_session");
    await bootstrap();
    $("activity").innerHTML =
      '';
    $("changes").innerHTML = "";
    $("tasks").innerHTML = "";
    $("change-count").textContent = "0";
  } catch (e) {
    toast(String(e));
  }
};
$<HTMLSelectElement>("mode").onchange = async () => {
  const previous = settings.mode;
  try {
    await invoke("save_settings", {
      settings: { ...settings, mode: $<HTMLSelectElement>("mode").value },
      apiKey: null,
    });
    await bootstrap();
  } catch (e) {
    $<HTMLSelectElement>("mode").value = previous;
    toast(String(e));
  }
};
$("undo").onclick = async () => {
  try {
    const n = await invoke<number>("undo");
    toast(`已恢复 ${n} 个文件；命令执行的修改不会撤销。`);
    $("changes").innerHTML =
      '<div class="panel-empty">最近一轮文件修改已撤销。</div>';
    $("change-count").textContent = "0";
    await bootstrap();
  } catch (e) {
    toast(String(e));
  }
};
document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach(
  (b) =>
    (b.onclick = () => {
      document
        .querySelectorAll("[data-tab]")
        .forEach((x) => x.classList.remove("active"));
      b.classList.add("active");
      ["activity", "changes", "tasks"].forEach(
        (id) => ($(id).hidden = id !== b.dataset.tab),
      );
    }),
);
$("model-label").onclick = () => showSettings();
$("toggle-inspector").onclick = () => {
  $("inspector").classList.toggle("collapsed");
};
document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.key === "n") {
    e.preventDefault();
    if (!busy) $("new").click();
  }
});
void bootstrap().catch((e) => {
  welcome();
  toast(String(e));
});
refreshIcons();
