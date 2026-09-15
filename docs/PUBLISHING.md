# GitHub 发布说明

## 公开范围

提交源代码、测试、锁文件、应用图标、构建配置和通用文档。排除依赖缓存、构建输出、SQLite 数据、本机验证日志、下载来源清单、恢复资料、LLM 配置和本地预览数据。

现有应用标识符 `com.yangjie.skillmanager` 是公开的包标识符，不是凭据；保留以兼容既有数据目录。仓库尚未选择开源许可证，发布者应在决定授权范围后添加 LICENSE。

## 构建与分发

1. 从全新检出执行 `pnpm install --frozen-lockfile`。
2. 执行 `pnpm test` 和 `pnpm build`。
3. 检查生成的 `.app`，按需要运行 `pnpm build:dmg`。
4. GitHub Actions 对 Apple Silicon 和 Intel 分别构建，上传保存 14 天的 ZIP Artifact。下载外层 Artifact 后，解压内层应用 ZIP，拖入“应用程序”。
5. 正式 Release 可手动上传对应架构的 ZIP/DMG，说明版本、架构、最低 macOS 12.0 和签名状态。当前工作流不会自动创建 Release。

GitHub 仓库 About 描述：

> Local desktop manager for AI coding skills and rules, with reversible project toggles, global installs, updates, and optional LLM assistance.

可用主题：`tauri`、`react`、`rust`、`skills`、`macos`。

自动构建使用 GitHub 临时只读令牌，不需要个人 GitHub Token 或 LLM Key。未来若启用 Apple 签名、公证，凭据应放入仓库 Secrets，不得写入文件。

## 首次发布验证（2026-09-15）

- 检查全部公开文件：未发现常见格式的真实访问令牌、私钥、带密码 URL 或个人绝对路径。此检查不构成对所有敏感信息的绝对保证。
- 忽略规则验证：本机 SQLite 备份、下载来源报告、验证日志、依赖缓存、预览数据、环境变量文件和 LLM 配置不会进入普通提交。
- 独立临时源码目录按锁文件安装依赖，并通过 TypeScript 与 Vite 生产构建；未使用历史依赖复制脚本。
- 21 项前端测试、147 项 Rust 测试通过。修复测试 HTTP 服务在 macOS 继承非阻塞连接模式的问题，并增加延迟请求回归测试。
- 本机 macOS `.app` 生产打包通过。DMG 命令、Intel 构建及 GitHub Actions 远端运行尚未在本次本机验证中确认。
