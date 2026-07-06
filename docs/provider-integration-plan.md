# Gmail、QQ、163 邮箱接入计划

本文记录 Outlook/Graph 已基本可用之后，桌面版下一批邮箱服务商的接入范围、技术路线、里程碑和验收标准。

本计划不恢复 Web 服务，不处理浏览器扩展，不做 SaaS。Gmail、QQ 邮箱和 163 邮箱都应接入现有本地优先架构，复用账号库存、SQLite 邮件缓存、刷新日志、重试队列、附件缓存、邮件列表、预览、导出和本地安全存储。

## 已查阅资料

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

优先路线：Gmail API + Google OAuth。

原因：

- Google 当前推荐第三方客户端使用 Google 账号登录，不推荐直接交出 Google 用户名和密码。
- Gmail API 支持消息列表、消息详情、基于 `historyId` 的增量同步，以及 Cloud Pub/Sub 推送通知。
- Gmail API scope 更细，但 `gmail.readonly`、`gmail.modify` 等邮件 scope 可能涉及敏感或受限权限，需要考虑 Google OAuth 应用验证。

兜底路线：Gmail IMAP + XOAUTH2。

仅在 Gmail API scope 审核或实现复杂度阻塞时使用。Gmail IMAP 支持 SASL XOAUTH2，但 IMAP/SMTP 使用的 `https://mail.google.com/` scope 权限很宽，合规要求更重。

首版目标：

- Google OAuth 授权、回调粘贴和 refresh token 加密保存。
- 使用 `messages.list` + `messages.get` 完成首次同步。
- 保存 Gmail `message.id`、`threadId`、`labelIds`、`historyId`、发件人、收件人、主题、正文、附件和接收时间。
- 基于 `historyId` 做增量同步。
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

- provider-specific JSON 设置，用于 Gmail `historyId`、Gmail label 同步状态、服务商能力探测结果和服务商警告。
- 后续如果做账号级 SMTP，再补账号级 SMTP host/port。

### OAuth

Gmail OAuth 不能直接复用 Microsoft OAuth 地址，需要 provider-specific 实现：

- Google 授权 URL。
- Google token URL。
- 桌面版继续使用当前“打开授权页 -> 粘贴回调 URL”的流程。
- 首版 scope 建议：
  - 只读 MVP：`https://www.googleapis.com/auth/gmail.readonly`
  - 需要远端标记/删除：`https://www.googleapis.com/auth/gmail.modify`
  - IMAP XOAUTH2 兜底：`https://mail.google.com/`

### 刷新与远端操作适配器

将刷新和远端操作整理为 provider adapter：

- `fetch_recent_messages(account, folder, top)`
- `download_attachment(account, message_id, attachment_id)`
- `mark_message_read(account, folder, message_id, is_read)`
- `delete_message(account, folder, message_id)`
- `discover_folders(account)`
- `refresh_delta(account)`，仅服务商支持时实现

适配器列表：

- Outlook Graph adapter：保留现有 Graph 逻辑。
- Generic IMAP adapter：保留现有 IMAP 逻辑。
- Gmail API adapter：新增。
- QQ adapter：IMAP preset + QQ 授权码提示 + 文件夹验证。
- 163 adapter：IMAP preset + 163 授权密码提示 + 文件夹验证。

### UI

账号管理：

- provider selector：Outlook、Gmail、QQ、163、Custom IMAP。
- provider-aware 凭据表单：
  - Outlook：Microsoft OAuth / IMAP OAuth。
  - Gmail：Google OAuth。
  - QQ：邮箱 + 授权码 + IMAP preset。
  - 163：邮箱 + 客户端授权密码 + IMAP preset。
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

- provider 自动识别
- provider registry 默认配置
- Gmail API 响应归一化
- IMAP provider preset 归一化
- 文件夹映射
- 附件解析继续 provider-neutral

Mock 网络集成测试：

- Gmail 首次同步
- Gmail `historyId` 增量同步
- QQ/163 IMAP raw MIME 同步
- IMAP 标记已读/未读和删除失败进入重试队列
- provider-specific 凭据错误归类

人工验证：

- Gmail OAuth 测试用户授权。
- Gmail 首次同步和增量同步。
- QQ 真实账号 IMAP 授权码刷新。
- 163 真实账号 IMAP 授权密码刷新。
- 三类服务商的附件下载。
- 三类服务商的标记已读/未读和删除。

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
- 刷新、附件下载、远端标记和远端删除已集中到 provider adapter 路由。Gmail adapter 在 P9.1 前会返回明确的未接入错误，不再误走 Graph/IMAP。
- 已补充前端导入 parser 测试、Rust provider registry 测试和数据库导入 preset 测试；`pnpm test`、`pnpm build`、`cargo test` 均已通过。

### P9.1 Gmail OAuth and Gmail API Read Sync

目标：把 Gmail 作为一等 OAuth 服务商接入，先完成只读同步。

交付：

- Google OAuth URL 生成和 token exchange。
- Gmail 账号保存流程。
- Gmail API 首次同步，使用 `messages.list` 和 `messages.get`。
- Gmail 邮件归一化写入现有本地缓存。
- Gmail 附件元数据和下载。

验收：

- Gmail 测试账号可以授权并保存。
- 刷新当前账号能拉取 Gmail 收件箱最新邮件。
- 邮件预览和附件下载可用。
- 本地搜索、排序、筛选、分页、导出和验证码复制可用于 Gmail 邮件。

### P9.2 Gmail Incremental Sync and Remote Actions

目标：让 Gmail 接近 Outlook/Graph 的同步效率和操作能力。

交付：

- 保存 Gmail `historyId`。
- 使用 Gmail `history.list` 做增量同步。
- `historyId` 过期或无效时自动回退首次同步。
- Gmail 标记已读/未读。
- Gmail 删除或移入垃圾箱行为与本地删除语义对齐。
- Gmail 远端操作失败接入重试队列。

验收：

- 重复刷新不会不必要地全量拉取邮件详情。
- 新邮件、已读状态变化和删除状态可以正确同步。
- Gmail 远端操作失败在 UI 中以 Graph/IMAP 同样方式展示和重试。

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

## 风险和待确认问题

- Gmail API scope 可能需要 OAuth 应用验证，尤其是公开给更多用户使用时。
- Gmail push notifications 依赖 Cloud Pub/Sub，不是纯本地桌面方案。
- QQ 和 163 授权码流程可能变化，需要真实账号确认。
- QQ 和 163 可能限制 IMAP 登录频率，或要求先在网页登录开启 IMAP。
- 文件夹名和 special-use flags 在不同邮箱中差异明显，必须真实账号验证。
- 远端删除语义不同：Gmail API 有 trash/delete 区别，IMAP delete 通常是 `\Deleted` + expunge。
- 当前 SMTP 转发偏应用全局配置；如果以后需要按账号发信，可能需要账号级 SMTP 配置。
