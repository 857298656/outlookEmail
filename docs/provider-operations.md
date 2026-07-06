# 多服务商接入故障排查与验收清单

本文档用于 P9.6 的真实账号验收和故障定位。当前范围只覆盖桌面版本地能力：账号导入、OAuth/IMAP 凭据、刷新同步、附件、远端标记/删除、重试队列和批量操作。

## 服务商速查

| 服务商 | provider | 凭据 | 默认接入 | 关键限制 |
| --- | --- | --- | --- | --- |
| Outlook / Microsoft | `graph` | Microsoft OAuth refresh token | Graph API | 需要 Microsoft Client ID 和 OAuth callback URL。 |
| Gmail | `gmail` | Google OAuth refresh token | Gmail API | 需要 `gmail.modify` scope；旧 `gmail.readonly` 授权需要重新授权后才能远端标记和移入垃圾箱。 |
| QQ 邮箱 | `qq` | IMAP/SMTP 授权码 | `imap.qq.com:993` | 必须先在网页端开启 IMAP/SMTP；不要填写 QQ 登录密码。 |
| 163 邮箱 | `netease_163` | 客户端授权密码或应用密码 | `imap.163.com:993` | 必须先在网页端开启客户端授权；不要填写网页登录密码。 |
| Custom IMAP | `imap_custom` | IMAP 密码或应用密码 | 手动 host/port | 需要确认服务商支持 TLS IMAP 和远端 UID 操作。 |

注册表能力元数据当前覆盖：读取邮件、附件下载、已读/未读、远端删除或移入垃圾箱、Gmail history 增量同步、IMAP 文件夹发现。Gmail 使用 `trash` 和 `history_sync`；QQ、163 和 Custom IMAP 使用 `remote_delete` 和 `imap_folders`。

批量导入支持自动按域名识别 Gmail、QQ、Foxmail、163，也支持显式 provider，例如：

```text
provider=qq----user@example.com----imap-auth-code
netease_163----user@custom-domain.test----client-auth-password
person@gmail.com----optional-password----google-client-id----refresh-token----remark
```

## 常见故障

| 现象 | 优先检查 | 处理方式 |
| --- | --- | --- |
| 账号显示“缺少凭据” | provider 是否正确、OAuth refresh token 或 IMAP 密码是否存在 | Outlook/Gmail 重新授权并保存；QQ/163/Custom IMAP 在账号设置里填入 IMAP 授权码/密码。 |
| Gmail 远端标记失败或 403 | OAuth scope 是否仍是旧 `gmail.readonly` | 重新生成 Gmail OAuth URL，完成授权并保存 refresh token。 |
| Gmail 刷新 401 / `invalid_grant` | refresh token 是否被撤销、Client ID 是否变更 | 使用同一个 Google Client ID 重新授权；必要时删除旧账号后重新保存。 |
| Gmail history 404 | Gmail `historyId` 已过期 | 当前实现会回退到全量同步；观察下一次 all 文件夹刷新是否恢复。 |
| QQ/163 登录失败 | IMAP 是否开启、授权码是否填入 IMAP 密码字段、是否填错网页登录密码 | 在网页端重新生成授权码/授权密码，确认账号未触发服务商安全限制后重试。 |
| IMAP 文件夹不进入垃圾箱/已删除 | special-use flags 或中文文件夹名与预设不一致 | 记录服务商实际文件夹名，补充 `classify_imap_mailbox` 映射后回归测试。 |
| 附件下载失败 | 消息是否已刷新并缓存 raw MIME 或 Gmail attachment id | 先刷新该账号；IMAP 需要本地已缓存 MIME，Gmail 需要可访问 Gmail API。 |
| 远端标记/删除失败 | 邮件详情里的远端同步失败面板、刷新管理/重试队列 | 使用“立即重试”；如果是永久凭据错误，先修复凭据再重试。 |
| 批量导入识别错误 | 导入预览里的服务商和凭据类型 | 给该行加显式 `provider=...` 或以 provider 作为首列。 |
| 代理/网络错误 | 账号代理链、全局网络、服务商访问限制 | 先用单账号刷新验证；失败会进入重试队列并按退避时间重试。 |

Gmail `HTTP 401/403`、`invalid_grant`、`insufficientPermissions`、scope 缺失，QQ 授权码错误，以及 163 客户端授权密码/网页登录密码错误都会归类为 `auth`，刷新失败汇总和重试退避会按凭据问题处理。

## 手工验收前置条件

- 使用单独测试分组，避免真实账号与生产账号混在一起。
- 桌面应用已完成本地密码解锁，数据库可写。
- Outlook/Gmail OAuth callback URL 与应用设置一致。
- QQ/163 已在网页端开启 IMAP，并生成一次性或客户端授权密码。
- 如需代理，账号主代理和备用代理链已配置。
- 每个服务商至少准备 3 封测试邮件：纯文本、HTML、带附件。
- 每个服务商至少准备可标记已读/未读和可删除/移入垃圾箱的测试邮件。

## Gmail 验收

1. 在账号授权弹窗选择 Gmail，填入 Google Client ID，生成 OAuth URL。
2. 完成授权后粘贴 callback URL 或 code，保存账号。
3. 刷新 Inbox，确认邮件列表、正文预览、HTML 正文、附件元数据可见。
4. 下载至少一个 Gmail 附件，确认文件名、MIME 类型和大小合理。
5. 执行标记已读、标记未读，确认本地状态更新且没有远端失败面板。
6. 删除一封测试邮件，确认行为是移入 Gmail Trash，不做永久删除。
7. 连续刷新 all 文件夹两次，确认第二次可复用 `gmail_history_id` 增量路径。
8. 人为撤销授权或改错 Client ID，确认刷新失败可进入失败汇总和重试队列。

## QQ 邮箱验收

1. 在 QQ 邮箱网页端开启 IMAP/SMTP，生成授权码。
2. 导入或创建账号，provider 选择 QQ 邮箱，IMAP host 应为 `imap.qq.com`，端口 993。
3. 使用授权码作为 IMAP 密码刷新 Inbox。
4. 确认 raw MIME 缓存可支持正文和附件解析。
5. 验证垃圾邮件、已删除或回收站文件夹是否能映射到应用的 Spam/Trash 类文件夹。
6. 执行已读/未读和删除操作；如果服务商拒绝 UID 操作，确认失败进入重试队列且 UI 可见。
7. 用批量导入预览检查 `@qq.com`、`@foxmail.com` 和 `provider=qq` 都识别为 QQ 邮箱。

## 163 邮箱验收

1. 在 163 邮箱网页端开启 IMAP/SMTP 或客户端授权，生成授权密码。
2. 导入或创建账号，provider 选择 163 邮箱，IMAP host 应为 `imap.163.com`，端口 993。
3. 使用授权密码作为 IMAP 密码刷新 Inbox。
4. 确认中文文件夹名可以按常见映射进入垃圾邮件或已删除类文件夹。
5. 验证 HTML 正文、纯文本正文、raw MIME 附件下载。
6. 执行已读/未读和删除操作；失败时确认重试队列和邮件详情失败面板可用。
7. 用批量导入预览检查 `@163.com` 和 `provider=netease_163` 都识别为 163 邮箱。

## 多服务商批量验收

1. 混合导入 Outlook/Gmail/QQ/163/Custom IMAP 测试账号，确认导入预览显示正确服务商和凭据类型。
2. 账号树、账号管理表、刷新管理表均应显示统一服务商徽标。
3. 使用账号筛选选择 Outlook、Gmail、QQ、163、IMAP，确认列表结果正确。
4. 刷新全部账号，确认失败账号按服务商出现在刷新管理汇总中。
5. 点击失败服务商汇总块，确认账号列表切换到失败账号并填入服务商搜索词。
6. 对跨服务商选中邮件执行批量已读/未读、删除、标签和导出，确认本地结果正确。
7. 对远端失败项执行立即重试和忽略，确认重试队列状态变化符合预期。

## 自动化回归覆盖

每次合并服务商接入相关改动前运行：

```powershell
pnpm test
pnpm build
$env:RUSTUP_HOME='E:\RustCache\.rustup'; $env:CARGO_HOME='E:\RustCache\.cargo'; $env:PATH='E:\RustCache\.cargo\bin;' + $env:PATH; cargo test
```

当前回归重点：

- `src/lib/importParser.test.ts` 覆盖批量导入 provider 字段、域名识别和 legacy 行格式。
- `src/lib/providerRegistry.test.ts` 覆盖前端 provider capability metadata、Gmail history/trash 能力和 QQ/163 IMAP 文件夹能力。
- `providers::tests::detects_mail_provider_registry_defaults` 覆盖后端 provider registry、别名和默认 IMAP preset。
- `db::project_tests::import_accounts_detects_mail_provider_presets` 覆盖导入后 QQ/163/Gmail provider、account_type 和 IMAP host/port 归一化。
- `providers::tests::normalizes_gmail_message_payload`、`maps_gmail_labels_to_cached_folders` 和 `maps_gmail_folder_targets_to_app_folders` 覆盖 Gmail 正文、附件、label 到本地文件夹映射。
- `providers::tests::compares_gmail_history_ids_numerically` 和 `db::project_tests::extracts_gmail_history_id_from_provider_sync_state` 覆盖 Gmail `historyId` 状态解析。
- `providers::tests::classifies_imap_special_use_and_common_mailbox_names` 覆盖 IMAP special-use 和常见中文文件夹名映射。
- `db::project_tests::classifies_provider_specific_credential_errors` 覆盖 Gmail、QQ、163 provider-specific 凭据错误归类。
- `db::project_tests::queues_failed_account_refresh_retry`、`queues_and_retries_failed_remote_mail_action`、`failed_remote_delete_keeps_message_visible_until_retry_success` 覆盖刷新失败、远端操作失败和删除回滚路径。

## 已知限制

- Gmail、QQ、163 的真实账号流程仍需要人工验收；自动化测试只能覆盖归一化、路由和本地重试逻辑。
- Gmail push notifications 依赖 Cloud Pub/Sub，不属于当前纯本地桌面验收范围。
- IMAP 删除语义因服务商不同可能表现为 `\Deleted` + expunge、移动到 Trash 或服务商拒绝 UID 操作。
- QQ/163 授权码流程可能随服务商策略变化，需要以真实账号网页端说明为准。
- 当前 SMTP 转发仍是全局配置；按账号发信或按服务商发信不在本轮 M9 范围内。
