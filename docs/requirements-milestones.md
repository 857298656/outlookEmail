# Requirements and Milestones

## 需求边界

本项目目标是用现代桌面技术复刻 `assast/outlookEmail` 的核心能力，并根据后续要求调整为：

- 只做桌面应用，不再做浏览器扩展。
- 使用 SQLite 作为本地数据库。
- 保持本地优先，单用户本机运行，不做 SaaS 多租户服务。
- 优先还原原项目的邮箱管理、账号池、邮件同步、转发、备份和自动化能力。
- 后续再补齐分享链接、外部集成、邮件操作增强、临时邮箱增强等周边能力。

### 当前取舍记录（2026-07-03）

- Web 服务、完整 HTTP API、浏览器扩展、SaaS/服务器部署能力暂不进入近期计划。
- 当前目标按“桌面版够用，优先补功能”推进；新增能力优先落在本地 Tauri command、SQLite 数据模型和桌面 UI。
- 已有数据结构但未形成完整功能的能力，例如 `account_aliases`、`email_share_links`、`proxy_url` 和 `fallback_proxy_url_*`，按桌面功能优先级逐步接上。
- 与 Web 服务强绑定的能力，例如浏览器扩展登录、Session/CSRF、对外 API Key、Docker 在线更新，先记录为暂缓项，不作为近期验收口径。

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
- 分组支持编辑、删除、排序、三级树展示、跨层级移动，删除时会提升子分组并迁移账号归属。
- 账号编辑页支持分配和保存标签。
- 账号别名增删改查、搜索命中和项目池优先使用首个别名。
- 账号库存支持多选批量删除、移动分组、开关转发、添加/移除标签和按选中账号导出。
- 刷新管理桌面视图支持统计、失败账号列表、单账号刷新、选中账号批量刷新、停止选中批量刷新、刷新重试队列、账号级刷新历史和刷新任务历史。
- 账号敏感信息查看需要本地密码二次验证，验证后只读显示账号密码、Client ID、Refresh Token 预览和 IMAP 密码。
- 账号/分组 HTTP 代理链配置，Graph、IMAP HTTP CONNECT、SMTP HTTP CONNECT 和 HTTP 转发通道按主代理、备用代理顺序故障切换。
- 账号授权编辑面板。
- Microsoft Graph OAuth 授权 URL 生成。
- OAuth code/callback token exchange。
- Graph 邮箱刷新和本地缓存。
- Graph 附件元数据同步和附件下载。
- 基础 IMAP TLS 登录、UID search/fetch、MIME 解析。
- IMAP 原始 MIME 本地缓存和附件解析下载。
- 邮件列表、邮件详情、附件入口。
- 刷新日志和账号刷新状态记录。
- 账号刷新失败会按账号、文件夹和拉取数量进入重试队列。

### 邮件操作

- 邮件搜索：支持按主题、发件人、收件人、摘要、正文搜索本地缓存。
- 高级搜索语法：支持 `from:`、`to:`、`subject:`、`body:`、`folder:`、`is:`、`has:`、`id:` 字段 token。
- 邮件筛选：支持按文件夹、已读/未读、是否有附件筛选。
- 邮件排序：支持按日期、主题、发件人、已读状态、附件和文件夹升降序排序。
- 邮件分页：支持固定页大小分页浏览缓存邮件。
- 邮件正文 HTML 通过清理和沙箱 iframe 安全渲染，不插入主应用 DOM 执行。
- 单封和批量标记已读/未读。
- 单封和批量删除。
- Graph 标记已读/未读和删除远端同步尝试。
- IMAP UID flag 标记已读/未读和删除远端同步尝试。
- Graph/IMAP 远端标记和删除失败会进入重试队列。
- Graph/IMAP 远端标记和删除失败会在邮件列表和详情中显示失败状态，并提供快速重试和忽略入口。
- 删除类远端失败会保留本地缓存邮件作为回滚展示；删除重试成功后再移除本地缓存。
- 设置页支持查看、手动重试和忽略远端操作失败队列项。

### 项目账号池

- 桌面项目管理视图。
- 支持从全部账号、指定分组或指定标签同步项目账号池。
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
- WebDAV 备份失败会进入重试队列。
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
- Cloudflare 临时邮箱支持按通道批量生成，并可为批量生成地址写入标签。
- 临时邮箱生成表单支持本地智能用户名生成。
- Cloudflare 地址批量导入按分块执行并显示导入进度。
- 临时邮箱支持标签保存、标签筛选、服务商筛选和关键字搜索。
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
  - `feat: retry failed refresh and backup jobs`
  - `feat: download cached IMAP attachments`
  - `feat: sandbox HTML message rendering`
  - `feat: add advanced mail search and sorting`
  - `feat: surface remote mail sync failures`
  - `feat: preserve failed remote deletes`
  - `feat: sync project pools by tags`
  - `feat: add account alias management`
  - `feat: add proxy config and failover`
  - `feat: add SMTP proxy failover`
  - `feat: enhance group management`
  - `feat: add account batch operations`
  - `feat: add refresh management view`
  - `feat: protect account secret reveal`
  - `feat: add temp email labels and cloudflare batch generation`
  - `feat: add temp email smart names and import progress`

## 未完成

### 桌面版功能优先级

近期按桌面实际使用价值排序，先补直接影响账号管理、收信稳定性和批量操作效率的功能。

P0：已补齐

- 当前 P0 项已完成，后续缺口从 P1 开始推进。

P1：补齐常用体验

- 邮件详情增强：全部附件打包下载、原始邮件 raw 查看、更多附件错误提示和下载状态。
- IMAP 增强：XOAUTH2、文件夹发现、特殊文件夹映射，不再只依赖固定 `Junk` / `Deleted` 名称。
- 本地保留管理：缓存统计、清理入口、关闭确认、新邮件同步提示和更明确的本地/远程状态。
- 调度与重试可观测性：独立任务历史仪表盘、更细错误分类、转发失败退避和通道熔断。

P2：后续增强

- 邮件分享能力的桌面化方案：优先考虑本地导出记录、可撤销导出、过期策略；公开分享链接需要 Web 服务支撑，暂不作为近期目标。
- 皮肤系统：如果桌面 UI 稳定后仍需要，再评估 zip/Git 皮肤包或更简单的本地主题方案。
- 更完整的端到端测试：覆盖导入、刷新、邮件操作、临时邮箱、备份恢复和重试队列。

### 暂缓项

- 完整 Web 服务和对外 HTTP API。
- Chrome/Edge 浏览器扩展。
- Docker/服务器部署、Watchtower 在线更新和远程多设备访问。
- Web 登录、Session/CSRF、API Key 鉴权、面向公网部署的安全体系。

### IMAP 完整能力

- IMAP XOAUTH2。
- 更完整的文件夹发现和特殊文件夹映射。

### 分享与外部集成增强

- 邮件分享链接（依赖 Web 服务的公开访问形态暂缓，先考虑桌面本地导出记录）。
- 可撤销导出记录和过期策略。
- 面向外部脚本的本地 HTTP API（暂缓）。
- 账号凭据导出策略和二次确认流程。

### 项目池增强

- 面向外部脚本的本地 HTTP API（暂缓）。
- 领取 token 的外部校验和更严格租约策略。

### 生产级强化

- 转发失败的更细粒度退避策略和通道熔断。
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
- 完成邮件缓存、邮件列表、详情、Graph 附件下载和 IMAP 缓存 MIME 附件下载。

### M3：项目账号池，已完成

- 完成项目创建。
- 完成全部账号、分组和标签范围的账号池同步。
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
- 剩余增强：更多服务 API 兼容、失败错误可视化。

### M6：邮件操作核心，已完成

- 目标：补齐原项目常用邮件操作。
- 已交付：标记已读/未读、删除、批量操作、搜索、筛选、分页。
- 已交付：Graph `PATCH`/`DELETE` 和 IMAP UID flag 远端同步尝试。
- 已交付：Graph/IMAP 远端标记和删除失败重试队列。
- 已交付：IMAP 同步缓存 raw MIME，并支持从缓存 MIME 中解析下载附件。
- 已交付：邮箱和临时邮箱 HTML 正文使用清理后的沙箱 iframe 渲染。
- 已交付：高级搜索语法和多字段升降序排序。
- 已交付：标记已读/未读远端失败在邮件列表和详情中可视化，并支持快速重试或忽略。
- 已交付：删除类远端失败会保留本地缓存邮件并在邮件列表和详情中可视化；重试成功后清理本地缓存。
- 剩余增强：无核心缺口，后续按真实使用反馈继续优化邮件操作体验。

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
- 已交付：账号刷新失败和 WebDAV 备份失败任务级重试。
- 剩余交付物：调度历史独立仪表盘、端到端测试。
- 风险点：自动化任务会触发真实网络请求，需要更强的失败隔离和可观测性。

## 当前推荐下一步

继续按“桌面版够用，优先补功能”推进。下一批建议先做邮件详情增强，然后推进 IMAP 增强；Web 服务和浏览器扩展暂不处理。
