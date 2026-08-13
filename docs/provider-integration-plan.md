# Gmail、QQ、163 邮箱接入计划

本文记录 Outlook/Graph 已基本可用之后，桌面版下一批邮箱服务商的接入范围、技术路线、里程碑和验收标准。

本计划不恢复 Web 服务，不处理浏览器扩展，不做 SaaS。Gmail、QQ 邮箱和 163 邮箱都应接入现有本地优先架构，复用账号库存、SQLite 邮件缓存、刷新日志、重试队列、附件缓存、邮件列表、预览、导出和本地安全存储。

当前实现决策：Gmail **只走 IMAP 应用专用密码**（`imap.gmail.com:993`），**不生成 Google OAuth URL、不做 Gmail API 同步、不保存 Gmail refresh token**。后端 `normalize_oauth_provider` 和 `exchange_oauth_code_for_provider` 会对 `gmail`/`google` 直接返回错误。OAuth 当前仅支持 Microsoft Graph 和 Outlook IMAP OAuth（`graph` / `imap` provider）。

**实现状态（2026-07-08）：** P9.0–P9.6 代码与文档已落地；Gmail/QQ/163 均复用通用 IMAP 适配器。剩余缺口主要是真实 Gmail/QQ/163 测试账号的人工验收，不是功能开发 backlog。

## 已查阅资料

以下链接主要供**暂缓的 Gmail API/OAuth 路线**参考；当前实现不调用这些 API。

- Gmail API scopes: https://developers.google.com/workspace/gmail/api/auth/scopes
- Gmail API list messages: https://developers.google.com/workspace/gmail/api/guides/list-messages
- Gmail API synchronization: https://developers.google.com/workspace/gmail/api/guides/sync
- Gmail API push notifications: https://developers.google.com/workspace/gmail/api/guides/push
- Gmail IMAP / POP / SMTP and XOAUTH2: https://developers.google.com/workspace/gmail/imap/imap-smtp
- Gmail XOAUTH2 protocol: https://developers.google.com/workspace/gmail/imap/xoauth2-protocol
- Gmail 客户端接入帮助: https://support.google.com/mail/answer/7126229?hl=zh-Hans
- QQ 邮箱 POP/IMAP/Exchange 设置入口: https://service.mail.qq.com/detail/0/310

网易 163 邮箱公开帮助页本次未能稳定检索到可引用页面。本文中的 163 默认配置先作为实现预设记录，后续开发前必须使用真实 163 账号人工验证。

## 接入策略

### Gmail

当前路线：Gmail IMAP + Google 账号应用专用密码。

原因：

- 与 QQ/163 一样复用现有 IMAP 适配器，当前阶段不处理 Google OAuth 登录和应用验证。
- Gmail 用户需要先开启 IMAP，并使用 Google 账号应用专用密码；不要填写 Google 网页登录密码。
- 导入行的 password 字段保存到 IMAP 密码字段，provider 保持 `gmail`，account_type 归一为 `imap`。

暂缓路线：Gmail API + Google OAuth。

仅在后续明确恢复 OAuth 登录时使用。届时需要重新评估 Google OAuth 应用验证、scope、PKCE、refresh token 保存和 Gmail API 增量同步。

首版目标：

- Gmail 域名自动识别为 provider `gmail`。
- 默认 IMAP host/port 为 `imap.gmail.com:993`。
- 应用专用密码保存到加密 IMAP 密码字段。
- 刷新、正文解析、附件下载、已读/未读和删除操作复用 IMAP 适配器。
- 邮件进入现有 SQLite 缓存后，复用列表、搜索、预览、验证码复制、导出和附件下载。

### QQ 邮箱

优先路线：IMAP + QQ 邮箱客户端授权码。

预设配置，开发前需要真实账号验证：

- provider id: `qq`
- IMAP host: `imap.qq.com`
- IMAP SSL port: `993`
- SMTP host: `smtp.qq.com`
- SMTP SSL port: `465`
- 登录用户名：完整 QQ 邮箱地址
- 密钥：QQ 邮箱客户端授权码，不是 QQ 登录密码

首版目标：

- 账号设置里提供 QQ 邮箱选项和默认 IMAP 配置。
- 导入账号时识别 `@qq.com` 和 `@foxmail.com`。
- 复用现有 IMAP 刷新、MIME 解析、附件缓存下载、UID 标记已读/未读和删除。
- 验证 QQ 邮箱文件夹发现和特殊文件夹映射。

### 163 邮箱

优先路线：IMAP + 163 客户端授权密码。

预设配置，开发前需要真实账号验证：

- provider id: `netease_163`
- IMAP host: `imap.163.com`
- IMAP SSL port: `993`
- SMTP host: `smtp.163.com`
- SMTP SSL port: `465`
- 登录用户名：完整 163 邮箱地址
- 密钥：163 客户端授权密码或应用密码，不是网页登录密码

首版目标：

- 账号设置里提供 163 邮箱选项和默认 IMAP 配置。
- 导入账号时识别 `@163.com`。
- 复用现有 IMAP 刷新、MIME 解析、附件缓存下载、UID 标记已读/未读和删除。
- 验证 163 中文文件夹名、垃圾邮件、已删除等特殊文件夹映射。

## 共用开发项

### 服务商注册表

新增 provider registry，前后端统一使用：

- provider id: `outlook`、`gmail`、`qq`、`netease_163`、`imap_custom`
- 显示名称
- 凭据类型：OAuth、IMAP 授权码、IMAP 密码
- 默认 IMAP host/port/TLS
- 默认 SMTP host/port/TLS
- 文件夹映射规则
- 支持的远端操作
- OAuth token exchange 类型

该注册表应逐步替代 UI、刷新路由和导入逻辑里写死的 Outlook/Graph/IMAP 文案。

### 账号模型

当前账号模型大概率可以支撑本轮接入，不需要一开始做破坏性迁移，因为已经有：

- `provider`
- `account_type`
- 加密账号密码
- 加密 refresh token
- IMAP host、port、password
- 账号状态、刷新状态、标签、别名、代理配置

可能需要补充：

- provider-specific JSON 设置，用于服务商能力探测结果和服务商警告（**暂缓** Gmail `historyId`/label 同步状态）。
- 后续如果做账号级 SMTP，再补账号级 SMTP host/port。

### OAuth

桌面 OAuth 当前**仅服务 Microsoft**：

- `graph` provider：Microsoft Graph OAuth v2，scope 含 `Mail.Read` / `Mail.ReadWrite`。
- `imap` provider：Outlook IMAP OAuth，scope 含 `IMAP.AccessAsUser.All`；登录时使用 IMAP XOAUTH2。
- Gmail、QQ、163、Custom IMAP **不使用 OAuth**；凭据分别写入 IMAP 应用专用密码或授权码/授权密码字段。

**暂缓：** Google OAuth / Gmail API / Gmail XOAUTH2。若后续重新启用，需要单独评估 scope、应用验证、PKCE 和增量同步方案；当前代码库中不存在 Gmail API 调用。

### 刷新与远端操作适配器

将刷新和远端操作整理为 provider adapter：

- `fetch_recent_messages(account, folder, top)`
- `download_attachment(account, message_id, attachment_id)`
- `mark_message_read(account, folder, message_id, is_read)`
- `delete_message(account, folder, message_id)`
- `discover_folders(account)`
- `refresh_delta(account)`，仅服务商支持时实现

适配器列表（当前实现）：

- Outlook Graph adapter：Microsoft Graph 刷新、附件、远端标记/删除。
- Generic IMAP adapter：Gmail、QQ、163、Custom IMAP、Outlook IMAP OAuth 共用；含 TLS 登录、UID fetch、raw MIME 缓存、XOAUTH2（仅 Outlook IMAP OAuth）、文件夹发现、UID 标记/删除。
- 163 特例：连接后发送 IMAP `ID` 命令（`imap.163.com` / `imap.126.com` / `imap.yeah.net`），满足网易客户端标识要求。

### UI

账号管理：

- provider selector：Outlook、Gmail、QQ、163、Custom IMAP。
- provider-aware 凭据表单：
  - Outlook：Microsoft OAuth；可选 Outlook IMAP OAuth（Client ID + OAuth 链接）。
  - Gmail：邮箱 + IMAP 应用专用密码 + `imap.gmail.com:993` preset。
  - QQ：邮箱 + IMAP 授权码 + `imap.qq.com:993` preset。
  - 163：邮箱 + 客户端授权密码 + `imap.163.com:993` preset。
  - Custom IMAP：手动 host/port/password。
- provider-specific 帮助文案和错误提示。

邮箱页：

- 账号树筛选增加 Gmail、QQ、163。
- 账号行显示服务商名称。
- 邮件列表刷新按钮继续只刷新当前选中账号。

### 导入

账号导入需要支持显式 provider 字段和自动识别：

- `gmail.com`、`googlemail.com` -> Gmail
- `qq.com`、`foxmail.com` -> QQ
- `163.com` -> 163
- 其他域名 -> Custom IMAP 或进入人工补全

QQ/163 导入行里的 password 字段应解释为客户端授权码或应用密码。

### 测试

单元测试：

- provider 自动识别与 registry 默认配置
- IMAP provider preset 归一化（Gmail/QQ/163 host/port/account_type）
- IMAP 文件夹映射（special-use、英文、中文、`[Gmail]/Spam`）
- provider-specific 凭据错误归类（Gmail 应用专用密码、QQ 授权码、163 授权密码）
- provider-aware 接入就绪判断
- 附件解析与 mail list preview 格式化（provider-neutral）

人工验证（仍待完成）：

- Gmail IMAP 应用专用密码刷新、附件、已读/未读、删除。
- QQ 真实账号 IMAP 授权码刷新与文件夹映射。
- 163 真实账号 IMAP 授权密码刷新与中文文件夹映射。
- 多服务商批量导入、筛选、刷新失败汇总与跨服务商邮件操作。

## 里程碑拆分

### P9.0 Provider Foundation

目标：让现有 Outlook/IMAP 实现具备 provider-aware 基础，不改变当前可见行为。

交付：

- provider registry。
- 后端刷新路由改成 provider adapter。
- 账号表单根据 provider 填充默认配置。
- 导入 parser 支持显式 provider 字段。
- 现有 Outlook Graph 和通用 IMAP 行为保持不变。

验收：

- Outlook Graph 账号刷新仍可用。
- 现有 IMAP 账号刷新仍可用。
- 现有测试通过。
- UI 不再把 Gmail/QQ/163 误标成 Outlook。

进展（2026-07-06）：

- 已新增前后端 provider registry，覆盖 `graph`/Outlook、`gmail`、`qq`、`netease_163`、`imap` 和 `imap_custom`。其中 `imap` 保留用于兼容现有 Outlook IMAP OAuth/通用 IMAP 路径，`imap_custom` 作为新导入的未知域名兜底。
- 账号导入 parser 已支持显式 provider 字段，例如 `provider=qq----user@example.com----auth` 和 `netease_163----user@custom.test----secret`；未显式指定时会按 `gmail.com`/`googlemail.com`、`qq.com`/`foxmail.com`、`163.com` 自动识别。
- QQ/163/Custom IMAP 导入行中的 `password` 字段已写入 IMAP 密钥位置；QQ 和 163 会自动填充默认 IMAP host/port。
- 账号树、账号库存、刷新管理和导入预览已改为显示真实服务商标签，支持按 Outlook/Gmail/QQ/163/IMAP 筛选，不再用 refresh token 状态把 Gmail/QQ/163 误标为 Outlook。
- 刷新、附件下载、远端标记和远端删除已集中到 provider adapter 路由；Gmail/QQ/163/Custom IMAP 走 Generic IMAP adapter，Graph 走 Graph adapter。
- 已补充前端导入 parser 测试、Rust provider registry 测试和数据库导入 preset 测试；`pnpm test`、`pnpm build`、`cargo test` 均已通过。

### P9.1 Gmail IMAP Provider

目标：把 Gmail 作为 IMAP 一等服务商接入。

交付：

- Gmail provider 选项与 `imap.gmail.com:993` preset。
- `@gmail.com` / `@googlemail.com` 导入自动识别。
- 账号设置应用专用密码提示；IMAP 密码字段保存 Google 应用专用密码。
- 后端拒绝 Gmail OAuth 请求，避免误走 Google token exchange。

验收：

- Gmail 测试账号开启 IMAP 后，可用应用专用密码刷新 Inbox。
- 邮件进入 SQLite 缓存后，列表、搜索、预览、附件下载可用。
- 本地搜索、排序、筛选、分页、导出可用于 Gmail 邮件。

进展（2026-07-08）：

- 前后端 registry 已将 Gmail 标记为 `imap_app_password`，account_type 归一为 `imap`。
- 导入、账号表单、批量预览、服务商徽标和筛选均已 provider-aware。
- 刷新/附件/远端操作经 IMAP adapter 路由；Gmail OAuth URL 生成与 token exchange 在代码层被禁用。
- **仍待真实 Gmail 测试账号人工验收。**

### P9.2 Gmail IMAP Remote Actions

目标：让 Gmail 通过 IMAP 具备与 Outlook IMAP 同级的操作能力。

交付：

- IMAP UID 标记已读/未读。
- IMAP `\Deleted` + expunge 删除语义。
- raw MIME 本地缓存与附件解析下载。
- 远端操作失败进入重试队列并在 UI 可视化。

验收：

- Gmail 标记已读/未读、删除可用，或失败时进入可视化重试。
- Gmail 应用专用密码错误会归类为 `auth` 并给出 setup hint。

进展（2026-07-08）：

- 已复用 Generic IMAP adapter 的 UID flag、删除、MIME 缓存和附件路径。
- provider-specific 凭据错误与 failure hint 已覆盖 Gmail 应用专用密码场景。
- **仍待真实 Gmail 测试账号验收远端 UID 操作。**

> **历史记录（已废弃，勿按此开发）：** 早期文档曾规划 Gmail OAuth + Gmail API + `historyId` 增量同步（P9.1/P9.2 OAuth 版本）。该路线已从代码中移除；若未来重新启用，需单独立项，不能假设现有实现存在 Google token endpoint 或 `users.messages.*` 调用。

### P9.3 QQ Mail IMAP Provider

目标：通过 IMAP preset 接入 QQ 邮箱。

交付：

- QQ provider 选项。
- QQ IMAP host/port preset。
- 授权码使用提示。
- `@qq.com` 和 `@foxmail.com` 导入自动识别。
- 文件夹发现和特殊文件夹映射验证。

验收：

- QQ 测试账号可以刷新收件箱邮件。
- 缓存 raw MIME 和附件解析下载可用。
- 标记已读/未读和删除可用，或者失败时进入可视化重试。
- 登录失败、IMAP 未开启、授权码错误、网络/代理失败尽量给出可区分错误。

进展（2026-07-06）：

- 已具备 QQ provider 选项、`imap.qq.com:993` preset、`@qq.com`/`@foxmail.com` 导入识别，以及通过通用 IMAP adapter 刷新、附件解析、UID 标记和删除的基础路径。
- 账号设置表单已显示 QQ 授权码提示，明确 IMAP 密码字段应填写 QQ 邮箱网页端生成的 IMAP/SMTP 客户端授权码，不是 QQ 登录密码。
- 批量导入弹窗已提示 QQ/163 导入行的 `password` 字段会保存为 IMAP 授权码/客户端授权密码，并支持 `provider=qq` 显式指定服务商。
- 尚未完成真实 QQ 邮箱账号人工验收，包括 IMAP 开启流程、授权码有效性、文件夹映射和远端 UID 操作。

### P9.4 163 Mail IMAP Provider

目标：通过 IMAP preset 接入 163 邮箱。

交付：

- 163 provider 选项。
- 163 IMAP host/port preset。
- 客户端授权密码使用提示。
- `@163.com` 导入自动识别。
- 中文文件夹名和特殊文件夹映射验证。

验收：

- 163 测试账号可以刷新收件箱邮件。
- 缓存 raw MIME 和附件解析下载可用。
- 标记已读/未读和删除可用，或者失败时进入可视化重试。
- 登录失败、IMAP 未开启、授权密码错误、网络/代理失败尽量给出可区分错误。

进展（2026-07-06）：

- 已具备 163 provider 选项、`imap.163.com:993` preset、`@163.com` 导入识别，以及通过通用 IMAP adapter 刷新、附件解析、UID 标记和删除的基础路径。
- 账号设置表单已显示 163 授权密码提示，明确 IMAP 密码字段应填写客户端授权密码或应用密码，不是网页登录密码。
- IMAP 文件夹分类已补充常见中文名称映射：`垃圾邮件`/`垃圾郵件`/`垃圾信` -> junkemail，`已删除`/`已删除邮件`/`垃圾箱`/`回收站` 等 -> deleteditems。
- 尚未完成真实 163 邮箱账号人工验收，包括 IMAP 开启流程、授权密码有效性、中文文件夹实际命名和远端 UID 操作。

### P9.5 Provider UX and Batch Operations

目标：让多服务商账号的日常使用足够顺畅。

交付：

- 账号树和账号管理页支持服务商筛选。
- 账号行显示服务商徽标或文本。
- 批量导入预览显示识别到的服务商和所需凭据类型。
- 刷新管理页按服务商汇总失败。
- 账号弹窗提供服务商设置帮助。

验收：

- 用户能按 Outlook/Gmail/QQ/163/IMAP 筛选账号。
- 用户能从 UI 判断服务商接入失败原因。
- 现有批量移动、标签、导出流程跨服务商可用。

进展（2026-07-06）：

- 批量导入弹窗已新增账号级预览，会按 parser 识别结果列出邮箱、服务商、所需凭据类型和备注，并保留顶部服务商数量汇总。
- 预览最多直接展示前 8 个账号，超出数量以汇总提示承接；导入保存逻辑仍复用现有批量导入 API。
- 刷新管理页已新增按服务商聚合的失败汇总，展示失败账号数和最常见错误；点击服务商汇总块会切到失败账号列表并填入服务商搜索词。
- 已新增统一服务商徽标组件，账号树、账号管理表、刷新管理表、批量导入预览和刷新失败汇总都会显示服务商短码与名称。
- 仍需真实多服务商批量操作验收。

### P9.6 Hardening and Documentation

目标：补齐服务商边界情况和操作文档。

交付：

- 服务商故障排查文档。
- 手工验收清单。
- provider detection、sync normalization、retry path 回归测试。
- 实现后更新 README 和架构文档。

验收：

- Gmail、QQ、163 都有可执行的设置步骤。
- 已知限制记录清楚。
- 标记里程碑完成前，构建和测试必须通过。

进展（2026-07-06）：

- 已新增 [`provider-operations.md`](provider-operations.md)，覆盖服务商速查、批量导入格式、常见故障处理、Gmail/QQ/163 手工验收、多服务商批量验收、自动化回归覆盖和已知限制。
- 前后端 provider registry 已补齐能力元数据，显式记录读取、附件、已读/未读、远端删除/移入垃圾箱、Gmail history 增量同步和 IMAP 文件夹能力，并增加前后端回归测试。
- 已补齐 provider-specific 凭据错误归类：Gmail HTTP 401/403、`invalid_grant`、`insufficientPermissions`/scope，QQ 授权码错误，以及 163 客户端授权密码错误会归入 `auth`，以便刷新失败汇总和重试退避按凭据错误处理。
- 刷新管理页的失败账号和服务商失败汇总已显示 provider-specific 处理建议，例如 Gmail 重新授权、QQ 开启 IMAP/SMTP 并使用授权码、163 使用客户端授权密码、网络/代理检查。
- 账号库存和刷新管理页已显示 provider-aware 接入就绪状态；OAuth 账号检查 Client ID 与 refresh token，QQ/163 检查 IMAP host/port 和授权码/授权密码，Custom IMAP 检查 host/port 和 IMAP 密钥。
- README 和架构文档已链接该操作文档；仍需要在拿到真实 Gmail/QQ/163 测试账号后执行人工验收清单。

## 风险和待确认问题

- Gmail OAuth / Gmail API / push notifications 已明确暂缓；当前 Gmail 只走 IMAP 应用专用密码。
- QQ 和 163 授权码流程可能变化，需要真实账号确认。
- QQ 和 163 可能限制 IMAP 登录频率，或要求先在网页登录开启 IMAP。
- 163 邮箱可能要求 IMAP `ID` 命令携带客户端标识；已实现但需真实账号验证。
- 文件夹名和 special-use flags 在不同邮箱中差异明显，必须真实账号验证。
- IMAP 删除语义不同：Gmail/QQ/163 通常表现为 `\Deleted` + expunge 或移动到 Trash，与 Graph `DELETE` 不同。
- `imap-proto` 通过 `vendor/imap-proto-0.10.2` 本地 patch 引入，构建依赖该 vendored crate。
- 当前 SMTP 转发偏应用全局配置；如果以后需要按账号发信，可能需要账号级 SMTP 配置。

## IMAP 兼容增强 backlog（暂缓）

PR1–PR3 已于 2026-07-08 落地（FLAGS/INTERNALDATE、SELECT 重试、通用 IMAP `ID`）。PR4 已于 2026-08-13 根据真实刷新异常落地：`UID SEARCH` 失败、在非空文件夹返回空集合，或 `UID FETCH` 返回的正文不完整时，自动回退 `SEARCH/FETCH` 序号路径并从响应保存 UID；回退不完整会保留原缓存。当前仅 PR5 保留为按需 backlog，详见 [`docs/provider-operations.md`](provider-operations.md)：

| 编号 | 内容 | 何时做 |
|------|------|--------|
| PR5 | UTF-7 解码 + LIST 模糊排名 fallback | 垃圾邮件/已删除等文件夹选不中、LIST 名为编码或非常规路径时 |

后续验收若仍出现文件夹识别问题，再按需实施 PR5。
