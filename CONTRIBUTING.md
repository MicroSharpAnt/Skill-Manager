# 参与开发

## 环境与命令

按 [README](README.md) 安装 macOS、Node.js 24、pnpm 10.12.3 和 Rust 工具链。

```sh
pnpm install --frozen-lockfile
pnpm dev
pnpm test
pnpm build
```

- `pnpm test:web`：Node 测试，覆盖分组、方案、翻译与差异计算。
- `pnpm test:rust`：在临时目录中验证项目恢复、全局管理和模拟 LLM 接口。
- `pnpm build:web`：TypeScript 检查与前端生产构建。
- `pnpm build`：生成当前 Mac 架构的 `.app`；`pnpm build:dmg`：生成 DMG。
- `pnpm dev:web`：仅界面预览，不能操作本机资源。

首次安装和编译需要联网。常规测试不需要真实 API Key；不要使用个人项目或真实 Skill 库做破坏性测试。联网示例需显式运行，具体见 README。

## 工程结构

- `src/`：React 页面、样式及前端业务逻辑。
- `src-tauri/src/`：Rust 文件操作、事务恢复、数据存储与桌面命令。
- `src-tauri/tests/`、`tests/`：后端与前端测试。
- `docs/`：功能与发布说明。

## 提交前

运行测试和桌面构建；检查 `git diff --cached`。保留两个锁文件，依赖变化时同步更新。不要提交 API Key、个人路径、数据库、日志、预览数据或安装产物。若使用本工具关闭了项目资源，先恢复，再提交。

修改启停、安装或恢复逻辑时，保留预检、预览、备份与冲突拒绝机制，并补充针对行为的回归测试。PR 说明应包含问题、行为变化和验证结果。

本仓库目前未声明开源许可证；公开托管不等于授予任意再分发许可。第三方依赖仍遵循各自许可证。
