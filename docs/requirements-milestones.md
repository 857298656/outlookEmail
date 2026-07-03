# Requirements and Milestones

## 需求边界

本项目目标是用现代桌面技术复刻 `assast/outlookEmail` 的核心能力，并根据后续要求调整为：

- 只做桌面应用，不再做浏览器扩展。
- 使用 SQLite 作为本地数据库。
- 保持本地优先，单用户本机运行，不做 SaaS 多租户服务。
- 优先还原原项目的邮箱管理、账号池、邮件同步、转发、备份和自动化能力。
- 后续再补齐分享链接、外部集成、邮件操作增强、临时邮箱增强等周边能力。

## 技术方案

- 桌面壳：Tauri 2
- 前端：React + Vite + TypeScript
- 后端：Rust Tauri commands
- 数据库：SQLite WAL mode
- 密钥保护：本地应用密码派生密钥，AES-GCM 加密敏感配置
- 邮件协议：Microsoft Graph OAuth + Graph API，基础 IMAP TLS
- 临时邮箱：GPTMail、DuckMail、Cloudflare Worker 通道
- 自动化：桌面进程内 scheduler
- 打包：Windows exe、MSI、NSIS installer

## 已完成

### 桌面基础

- Tauri 桌面应用骨架。
- React/Vite/TypeScript 主界面。
- SQLite 本地数据库初始化和 WAL 模式。
- 本地初始化、解锁、锁定流程。
- 敏感字段加密保存。
- Windows 可执行文件和安装包打包。

### 账号与邮箱

- 支持原项目账号导入格式。
- 账号、分组、标签管理。
- 账号授权编辑面板。
- Microsoft Graph OAuth 授权 URL 生成。
- OAuth code/callback token exchange。
- Graph 邮箱刷新和本地缓存。
- Graph 附件元数据同步和附件下载。
- 基础 IMAP TLS 登录、UID search/fetch、MIME 解析。
- 邮件列表、邮件详情、附件入口。
- 刷新日志和账号刷新状态记录。

### 邮件操作

- 邮件搜索：支持按主题、发件人、收件人、摘要、正文搜索本地缓存。
- 邮件筛选：支持按文件夹、已读/未读、是否有附件筛选。
- 邮件分页：支持固定页大小分页浏览缓存邮件。
- 单封和批量标记已读/未读。
- 单封和批量删除。
- Graph 标记已读/未读和删除远端同步尝试。
- IMAP UID flag 标记已读/未读和删除远端同步尝试。
- Graph/IMAP 远端标记和删除失败会进入重试队列。
- 设置页支持查看、手动重试和忽略远端操作失败队列项。

### 项目账号池

- 桌面项目管理视图。
- 支持从全部账号或指定分组同步项目账号池。
- 支持账号状态流转：`toClaim`、`claimed`、`success`、`failed`、`removed`。
- 支持领取、释放、成功、失败、移除、恢复。
- 项目统计和账号事件日志。

### 转发、备份、调度

- 账号级转发开关。
- SMTP 转发。
- Telegram Bot 转发。
- 企业微信 Webhook 转发。
- 转发日志和按渠道去重。
- 转发失败会记录到重试队列，并按退避时间重试。
- WebDAV 备份。
- 使用 SQLite `VACUUM INTO` 生成一致性备份快照。
- 支持从成功备份日志恢复本地 SQLite 快照。
- 恢复前执行 SQLite 完整性校验，并自动创建 `pre-restore` 安全快照。
- 应用内定时刷新、定时转发、定时备份。
- 设置页提供手动运行、配置保存、运行状态和日志查看。
- 手动和定时刷新/转发/备份统一记录到任务历史。
- 设置页展示任务类型、触发方式、状态、成功/失败数量、耗时和详情。
- 任务历史支持按任务类型、触发方式、状态、详情关键字筛选。
- 任务历史支持按当前筛选条件清理；无筛选时需要显式确认清空全部。
- 调度器会自动重试到期的失败队列项，手动/定时重试会进入任务历史。

### 临时邮箱

- GPTMail 临时邮箱生成、导入、刷新、删除。
- DuckMail 临时邮箱生成、导入、刷新、删除。
- Cloudflare Worker 邮箱通道管理。
- Cloudflare 邮箱域名、通道密码、默认通道配置。
- 临时邮箱消息刷新并缓存到 SQLite。
- 临时邮箱刷新失败会进入共享重试队列。
- 临时邮箱列表、消息列表、消息详情和刷新状态展示。
- 设置页支持 GPTMail 和 DuckMail API key 加密保存。

### 本地导出

- 选中缓存邮件导出为本地只读 HTML 文件。
- 邮件 HTML 导出对正文内容做转义输出，不执行原始邮件 HTML。
- 账号库存导出为 CSV 文件。
- 项目账号池导出为 CSV 文件。
- 导出文件统一写入应用数据目录下的 `exports` 目录。

### 交付和仓库

- 已绑定 GitHub 仓库：`https://github.com/857298656/outlookEmail.git`
- 已建立 `main` 分支。
- 已按批次提交并推送：
  - `chore: scaffold desktop workspace`
  - `feat: implement local desktop backend`
  - `feat: build desktop mailbox interface`
  - `docs: document desktop rebuild architecture`
  - `docs: add requirements and milestone plan`
  - `feat: add temp mail management`
  - `feat: add mail message operations`
  - `feat: add local export workflows`
  - `feat: add automation run history`
  - `feat: filter and clear automation runs`
  - `feat: restore local backup snapshots`
  - `feat: add failed operation retry queue`
  - `feat: retry failed temp mail refreshes`

## 未完成

### 临时邮箱增强

- AI 用户名生成。
- Cloudflare 批量生成和批量导入的流式进度。
- 临时邮箱标签和更细的筛选。
- 不同 GPTMail/DuckMail 部署版本的 API 兼容适配。
- 临时邮箱失败重试的批量策略和更细错误可视化。

### IMAP 完整能力

- IMAP 附件从缓存 raw MIME 中解析并下载。
- IMAP XOAUTH2。
- 更完整的文件夹发现和特殊文件夹映射。

### 邮件操作增强

- 高级搜索语法和多字段排序。
- 邮件正文 HTML 安全渲染策略。
- 远端操作失败的回滚提示和更细的错误可视化。

### 分享与外部集成增强

- 邮件分享链接。
- 可撤销导出记录和过期策略。
- 面向外部脚本的本地 HTTP API。
- 账号凭据导出策略和二次确认流程。

### 项目池增强

- 按标签同步项目账号池。
- alias email 优先级。
- 面向外部脚本的本地 HTTP API。
- 领取 token 的外部校验和更严格租约策略。

### 生产级强化

- 刷新、备份等任务级失败重试策略。
- 转发失败的更细粒度退避策略和通道熔断。
- 代理配置和故障切换。
- 更完整的端到端测试。
- 调度器任务历史的独立仪表盘增强。

## 里程碑

### M0：仓库和工程初始化，已完成

- 初始化 Git 仓库。
- 绑定 GitHub remote。
- 建立 `main` 分支并完成第一次推送。
- 建立基础 Tauri/Vite/Rust/SQLite 工程。

### M1：桌面基础和本地数据层，已完成

- 完成桌面壳、前端入口、Tauri command 桥接。
- 完成 SQLite schema。
- 完成本地密码、解锁和敏感字段加密。
- 完成账号、分组、标签基础模型。

### M2：账号导入和邮箱同步，已完成

- 还原原项目账号导入格式。
- 完成 Graph OAuth 和 Graph 邮箱同步。
- 完成基础 IMAP 邮箱同步。
- 完成邮件缓存、邮件列表、详情和 Graph 附件下载。

### M3：项目账号池，已完成

- 完成项目创建。
- 完成账号池同步。
- 完成账号领取和状态流转。
- 完成项目统计和事件记录。

### M4：自动化能力，已完成

- 完成 SMTP、Telegram、企业微信转发。
- 完成 WebDAV 备份。
- 完成应用内定时刷新、转发、备份。
- 完成设置页、手动触发和日志视图。

### M5：临时邮箱核心，已完成

- 目标：还原 GPTMail、DuckMail、Cloudflare 临时邮箱能力。
- 已交付：临时邮箱列表、生成/导入、消息刷新、删除、Cloudflare 通道设置。
- 已交付：GPTMail 和 DuckMail base URL/API key 配置，Cloudflare 通道密码加密保存。
- 已交付：临时邮箱刷新失败重试队列，支持手动和调度器自动重试。
- 剩余增强：AI 用户名、批量生成/导入流式进度、更多服务 API 兼容、失败错误可视化。

### M6：邮件操作核心，已完成

- 目标：补齐原项目常用邮件操作。
- 已交付：标记已读/未读、删除、批量操作、搜索、筛选、分页。
- 已交付：Graph `PATCH`/`DELETE` 和 IMAP UID flag 远端同步尝试。
- 已交付：Graph/IMAP 远端标记和删除失败重试队列。
- 剩余增强：IMAP 附件下载、HTML 安全渲染、高级搜索排序、远端失败回滚提示。

### M7：本地导出核心，已完成

- 目标：补齐分享导出和脚本集成能力。
- 已交付：邮件 HTML 导出、账号 CSV 导出、项目池 CSV 导出。
- 已交付：导出文件写入本地 `exports` 目录，邮件正文按文本转义输出。
- 剩余增强：邮件分享链接、本地 HTTP API、可撤销导出记录、账号凭据导出二次确认。

### M8：生产级稳定性，部分完成

- 目标：把功能从可用推进到长期稳定。
- 已交付：刷新、转发、备份的统一任务历史。
- 已交付：手动/定时触发类型、状态、成功/失败数量、耗时和详情记录。
- 已交付：任务历史筛选和按筛选条件清理。
- 已交付：本地备份快照恢复、恢复前完整性校验和安全快照。
- 已交付：邮件远端操作和转发失败重试队列，支持手动重试、忽略和调度器自动重试。
- 已交付：临时邮箱刷新失败重试队列，复用统一重试表和调度器退避。
- 剩余交付物：刷新/备份任务级重试策略、调度历史独立仪表盘、代理故障切换、端到端测试。
- 风险点：自动化任务会触发真实网络请求，需要更强的失败隔离和可观测性。

## 当前推荐下一步

继续推进 M8 生产级稳定性，同时补 M7+ 分享与外部集成增强。下一批建议做刷新/备份任务级重试、可撤销分享链接、调度历史独立仪表盘和代理故障切换；这些能力能把当前可用功能推进到更稳定的长期使用状态。
