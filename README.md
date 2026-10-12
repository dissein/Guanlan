# 观澜 · Guanlan

本地代码助手，使用 Rust、Tauri 2 和 TypeScript 构建。支持流式对话、文件读写、命令执行、工具审批和会话保存。

![观澜](docs/home.png)

## 下载与使用

从 [Releases](https://github.com/dissein/Guanlan/releases/latest) 下载 Windows x64 安装包 `Guanlan_<版本>_x64-setup.exe`，或直接运行便携版 `Guanlan_<版本>_x64.exe`。

打开「模型与设置」，选择服务商，填写 API 密钥和工作目录。点击「获取模型」后，可输入关键词筛选模型。支持兼容 OpenAI Chat Completions、流式输出和工具调用的服务；Ollama 可使用本地服务。

输入任务并发送。「执行模式」可以修改文件和执行命令，「规划模式」用于读取与分析。文件和命令操作默认逐次审批，可在设置中开启自动批准。界面材质可切换玻璃或纯色。

API 密钥保存于 Windows 凭据管理器，会话存于工作目录下的 `.guanlan/sessions/`。「撤销修改」恢复文件工具记录的修改，不撤销命令、Git 或网络操作。

## 功能

- 13 个工具：文件读取、写入、编辑、补丁、目录、检索、PowerShell、计划、提问、技能、网页读取和搜索。
- 模型列表获取、关键词筛选、多轮工具调用与推理内容展示。
- 文件差异、会话切换、停止执行及最近 20 轮文件工具撤销。
- 读取项目 `AGENTS.md`、`CLAUDE.md`、`GEMINI.md`；支持 `.agents/skills/` 和 `.guanlan-skills/` 中的技能。
- 网络请求支持环境变量与系统代理。

当前支持 Windows。命令使用当前用户权限执行；工作目录限制不等同于系统沙箱。

## 本地开发

需要 Node.js 24、Rust stable、Microsoft C++ Build Tools（Windows SDK 与桌面 C++ 工作负载）及 WebView2 Runtime。

```powershell
git clone https://github.com/dissein/Guanlan.git
cd Guanlan
npm ci
npm run desktop
```

```powershell
npm test
cargo test --locked -p guanlan-core
npm run package -- --ci
```

`npm run dev` 仅预览界面。本地安装包位于 `target/release/bundle/nsis/`。

## 自动发布

GitHub Actions 在 Windows runner 上运行测试并构建安装包和便携版。推送 `v<版本>` 标签后自动创建 Release，附带 SHA-256 校验文件；标签须与 `package.json`、`Cargo.toml` 和 `tauri.conf.json` 的版本一致。

```powershell
git tag v0.1.7
git push origin v0.1.7
```

也可以在 Actions 页面手动运行工作流。选择分支只生成构建产物，选择版本标签会发布 Release。

## 目录

```text
crates/guanlan-core/  Agent、流式解析、工具与会话
src-tauri/           桌面命令、凭据管理和打包
src/                 界面与资源
tests/               前端测试
.github/workflows/   GitHub 构建与发布
docs/                截图与版本说明
```

观澜由自己的 NanoCode 项目重写而来，运行时不依赖 Python。内置字体使用 Noto Sans SC，许可见 [OFL.txt](src/assets/OFL.txt)。
