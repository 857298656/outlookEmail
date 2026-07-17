# OutlookEmail Desktop

开源项目 [`assast/outlookEmail`](https://github.com/assast/outlookEmail) 的现代化桌面版复刻。本地优先、单用户本机运行，**不做 Web 服务、浏览器扩展或 SaaS 多租户**。

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面壳 | Tauri 2 |
| 前端 | React 19 + Vite + TypeScript |
| 后端 | Rust Tauri commands |
| 数据库 | SQLite WAL |
| 密钥保护 | 本地应用密码派生密钥，AES-GCM 加密敏感配置 |
| 邮件协议 | Microsoft Graph OAuth；TLS IMAP（含 Gmail/QQ/163 预设、Outlook IMAP OAuth XOAUTH2） |

## 功能概览

### 账号与邮箱

- 支持原项目账号导入格式，按域名或显式 `provider=` 自动识别服务商
- 账号、分组（三级树）与别名管理；分组支持跨层级移动与批量操作
- 账号库存多选：批量删除、移动分组和导出
- 刷新管理：单账号或全部账号刷新、状态与失败结果展示
- 账号敏感信息查看需本地密码二次验证；支持确认后导出账号凭据
- Provider 接入就绪检查（OAuth Client ID/refresh token 或 IMAP host/port/密钥完整性）
- HTTP 代理链配置，Graph/IMAP 按主代理、备用代理顺序故障切换

### 邮件收发与操作

- **Outlook / Microsoft Graph**：OAuth 授权、邮箱同步、附件元数据与下载
- **IMAP 通用能力**：TLS 登录、XOAUTH2、文件夹发现、MIME 本地缓存、附件解析下载
- 邮件搜索与高级语法：`from:`、`to:`、`subject:`、`body:`、`folder:`、`is:`、`has:`、`id:`
- 按文件夹、已读/未读、附件筛选；多字段排序；分页浏览
- 邮件正文 HTML 沙箱 iframe 安全渲染；列表摘要自动剥离 HTML/CSS 噪声
- 全屏预览弹窗：Raw 查看、分享和导出
- 单封/批量标记已读未读、删除；Graph/IMAP 远端同步失败会在任务结果中显示

### 调度

- 桌面进程内定时刷新邮箱
- 设置页可配置刷新间隔和每次拉取数量

### 临时邮箱

- GPTMail、DuckMail 临时邮箱生成、导入、刷新、删除
- Cloudflare Worker 邮箱通道管理、批量生成与分块导入
- 批量导入兼容 GPTMail 每行邮箱、DuckMail `邮箱----密码`、Cloudflare `[cloudflare:渠道名]` 分段，并按块显示导入进度
- Cloudflare 单次可批量生成 1–50 个地址并显示失败明细
- 临时邮箱服务商筛选、关键字搜索；消息刷新后缓存到本地 SQLite

### Markdown 工作台

- 侧边栏独立 Markdown 页面，提供树形文件夹与所见即所得编辑器
- SQLite 本地笔记库：新建、700ms 自动保存、删除、全文搜索和多级文件夹管理
- 支持 GFM 表格、任务列表、删除线、代码块、链接、LaTeX 和图片等常用内容
- 图片可通过选择、粘贴或拖放插入，并以内嵌数据保存到笔记中
- 原生打开并关联 `.md` / `.markdown` 文件，也可导入为独立 SQLite 副本
- 支持保存关联文件、另存为，以及按文件夹结构整目录导出 Markdown
- 笔记右键支持复制、删除，并可导出为 Markdown、HTML、PDF 或 PNG 图片

### 本地导出与分享

- 选中缓存邮件导出为本地只读 HTML
- 账号库存导出为 CSV
- 本地邮件分享记录
- 导出文件写入应用数据目录下的 `exports` 目录

### 其他

- 本地初始化、解锁、锁定流程
- 本地保留统计与清理（邮件、临时消息、附件、导出）
- 内置外观主题
- Windows 可执行文件和 NSIS 安装包打包

## 支持的邮箱服务商

| 服务商 | Provider | 凭据方式 | 说明 |
|--------|----------|----------|------|
| Outlook / Microsoft | `graph` | Microsoft OAuth | 需 Client ID 与 OAuth callback URL |
| Outlook IMAP | `imap` | OAuth XOAUTH2 | Microsoft OAuth，走 IMAP 协议 |
| Gmail | `gmail` | 应用专用密码 | `imap.gmail.com:993`，**不支持 Google OAuth** |
| QQ 邮箱 | `qq` | IMAP 授权码 | `imap.qq.com:993` |
| 163 邮箱 | `netease_163` | 客户端授权密码 | `imap.163.com:993`，连接时发送 IMAP `ID` |
| 自定义 IMAP | `imap_custom` | IMAP 密码 | 手动填写 host/port |

OAuth 仅支持 Microsoft（`graph` 与 Outlook `imap`）。Gmail/QQ/163 均走 IMAP 预设，不使用网页登录密码。

接入说明、故障排查与手工验收清单见 [`docs/provider-operations.md`](docs/provider-operations.md)。

## 环境要求

- Node.js 22+
- pnpm
- Rust stable + Cargo
- Tauri 2 所需的平台依赖

## 开发

```bash
pnpm install
pnpm tauri:dev
```

尚未安装 Rust 时，可仅验证前端：

```bash
pnpm install
pnpm build
pnpm test
```

Rust 单元测试：

```bash
pnpm cargo:test
```

## 构建

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
pnpm tauri:build
```

Windows 构建产物位于项目配置的 Rust target 目录：

- `release/outlook-email-desktop.exe`
- `release/bundle/nsis/OutlookEmail Desktop_*_x64-setup.exe`

## 数据存储

SQLite 数据库默认创建在平台应用数据目录；开发环境回退为项目根目录下的 `./outlook-email.sqlite`。Markdown 笔记、分类及本地文件关联路径同样保存在该数据库中。

敏感字段（密码、OAuth token、API key 等）使用本地应用密码派生的 AES-GCM 密钥加密保存。

## 邮件同步说明

**Graph 账号**：在账号授权面板生成 OAuth URL，将回调 URL 或 code 粘贴回应用后刷新账号。

**IMAP 账号**：填写 host、port 与密码（或授权码）。同步后在 SQLite 中缓存原始 RFC822 MIME，附件从本地缓存 MIME 解析下载。

**邮箱视图**：支持缓存邮件搜索、筛选、排序、分页；单封与批量操作会立即更新本地缓存并尝试远端同步，远端失败会在任务结果中显示。

**定时刷新**：工作区解锁后，调度器在桌面进程内运行；可在设置页配置刷新间隔与拉取数量，也可在邮箱页和账号页手动刷新。

## 文档

| 文档 | 内容 |
|------|------|
| [`docs/requirements-milestones.md`](docs/requirements-milestones.md) | 需求边界、已完成功能、里程碑计划 |
| [`docs/provider-integration-plan.md`](docs/provider-integration-plan.md) | Gmail/QQ/163 多服务商接入计划 |
| [`docs/provider-operations.md`](docs/provider-operations.md) | 故障排查与手工验收清单 |
| [`docs/architecture.md`](docs/architecture.md) | 运行时架构与模块说明 |

## 当前状态

M0–M9 代码已完成。Gmail/QQ/163 IMAP 预设、Provider 注册表、接入就绪检查与失败提示均已落地；临时邮箱分块导入和 SQLite 消息缓存已完成代码实现，**待真实账号与真实临时邮箱服务手工验收**。
