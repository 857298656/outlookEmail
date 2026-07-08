# Architecture

## Product boundary

This desktop build keeps OutlookEmail's local mailbox-management workflow and removes the old Flask web server and browser extension.

The application is a single-user local desktop app. It does not expose a public HTTP API by default.

## Runtime

- React/Vite renders the desktop UI.
- Tauri commands are the boundary between UI and native services.
- SQLite stores configuration, accounts, cached mail metadata/body content, temp-mail metadata, forwarding logs, backup logs, retry queue items, project state, share exports, and audit logs.
- Secrets are encrypted with AES-GCM using a key derived from the local app password.

## Implemented modules

- Local setup/unlock/lock flow
- SQLite schema and default data
- Account import with legacy delimiter support and provider-aware auto-detection
- Group and tag management with account tag assignment
- Cached message workspace with full-screen mail preview dialog
- Mail list preview text formatting (`src/lib/mailPreview.ts`) that strips HTML/CSS noise from snippets
- Settings persistence and local appearance themes (preset skins + accent color)
- Microsoft Graph OAuth and mailbox refresh (OAuth limited to `graph` and Outlook `imap` providers)
- Generic TLS IMAP mailbox refresh for Gmail/QQ/163/Custom IMAP and Outlook IMAP OAuth (XOAUTH2)
- Cached message field-token search, filters, multi-field sorting, pagination, batch read/unread/delete actions, and remote failure surfacing
- Sandboxed HTML body rendering for cached mailbox and temp-mail messages
- Local HTML/CSV exports for cached mail, account inventory, and project account pools
- Confirmed CSV export for selected account secrets after local password verification
- Local mail share records with generated HTML files, expiration, and revocation
- GPTMail, DuckMail, and Cloudflare temp-mail management
- SMTP, Telegram, and WeCom forwarding for cached messages
- Retry queue for failed account refreshes, remote mail actions, forwarding sends, temp-mail refreshes, and WebDAV backups
- WebDAV backup from a consistent SQLite snapshot with local restore
- Background scheduler inside the desktop process
- Unified automation run history with filtering and clearing; dedicated Automation dashboard view
- Desktop project account pools
- Provider registry, readiness checks, failure hints, and credential error classification
- Windows executable, MSI, and NSIS bundle generation

## Provider behavior

- Account providers are normalized through a shared provider registry. Current ids are `graph`/Outlook, `gmail`, `qq`, `netease_163`, `imap`, and `imap_custom`; `imap` is retained for the existing Outlook IMAP OAuth/generic IMAP path, while `imap_custom` is the new unknown-domain import fallback.
- Graph uses Microsoft OAuth v2 and Graph `/me/mailFolders/{folder}/messages` calls. OAuth auth URL generation and token exchange accept `graph` and Outlook `imap` providers only.
- Gmail uses the generic TLS IMAP adapter with preset `imap.gmail.com:993` and a Google account app password stored as the encrypted IMAP password. Backend rejects Gmail/Google OAuth requests with an explicit error.
- Gmail attachment downloads use the same IMAP raw-MIME cache path as other IMAP providers.
- Graph attachments are listed during sync and downloaded to the local app data `attachments` directory on demand.
- Graph message read/unread and delete actions update the local cache and attempt Microsoft Graph `PATCH`/`DELETE` synchronization. Failed remote actions are stored in `retry_queue` with the original account, folder, provider message id, action, error, attempt count, and next attempt time. Failed deletes keep the cached message visible as a rollback surface until the delete retry succeeds.
- IMAP uses TLS login, UID search/fetch, and MIME parsing. Outlook IMAP OAuth accounts authenticate with XOAUTH2 using Microsoft refresh tokens.
- IMAP folder discovery uses special-use flags where available and falls back to common English and Chinese mailbox names for inbox/junkemail/deleteditems, including `[Gmail]/Spam`, QQ/163-style folders, and Chinese names such as `垃圾邮件` / `已删除邮件` / `回收站`.
- NetEase IMAP accounts (`netease_163`, `imap.163.com`, `imap.126.com`, `imap.yeah.net`) send an IMAP `ID` command after login with a fixed client identity string.
- IMAP sync stores the raw RFC822 MIME for cached messages so attachment downloads can be resolved locally from SQLite-backed cache data without another network fetch.
- IMAP message read/unread and delete actions use UID flag updates; delete applies `\Deleted` and expunges the selected mailbox. Failed IMAP flag/delete operations use the same retry queue.
- Temp-mail providers use configurable GPTMail and DuckMail HTTP APIs plus Cloudflare Worker admin channels.
- Temp-mail messages are normalized and cached in `temp_email_messages` for local browsing. GPTMail, DuckMail, and Cloudflare refresh failures update the temp mailbox status and queue a `temp_refresh` retry item.
- HTML message bodies are sanitized for active content and rendered in a no-script sandboxed iframe with a restrictive content security policy instead of being inserted into the main React DOM.
- Mail list rows and preview headers use `formatMailPreview` / `formatMessageListPreview` to strip tags, inline CSS blocks, and HTML entities from snippet text before display.
- The mailbox detail experience is a modal preview dialog with read/unread actions, raw MIME view, share/export shortcuts, remote failure panel, and active share records.
- Mailbox search runs against the local SQLite cache and supports free text plus field tokens for sender, recipients, subject, body, folder, read state, attachments, and provider message id. Sorting is validated server-side before being applied to SQL.
- Cached message rows are joined with the latest pending or failed same-folder `mail_mark` / `mail_delete` retry item so the UI can show remote-sync failures and expose retry or dismiss actions without duplicating failure state.
- Refreshed messages are upserted into SQLite and read by the workspace UI.
- Refresh failures are recorded on the account and in `refresh_logs`; failed account refreshes queue a `refresh_account` retry item with account, folder, and page size.
- Provider-aware routing now selects Graph or IMAP before refresh, attachment download, remote read/unread, and delete actions. Gmail routes to IMAP.
- Provider readiness shown in the account inventory and refresh management views is derived from non-secret account flags such as Client ID presence, refresh token presence, IMAP host/port, and IMAP password presence; it does not decrypt or display stored secrets.
- Forwarding is controlled by a per-account `forward_enabled` flag and deduplicated through `forwarding_logs`. Failed SMTP/Telegram/WeCom sends are queued with message id and channel for later replay.
- Backups are created with SQLite `VACUUM INTO`, stored locally under the app data backup directory, then uploaded with WebDAV `PUT`. Failed backup attempts queue a `backup_job` retry item.
- Restores are limited to successful backup log entries that resolve to local `.sqlite` snapshots under the app backup directory. The restore command validates the snapshot with SQLite `integrity_check`, creates a pre-restore safety snapshot, replaces the current database file set, reopens SQLite, and audits the restore.
- Scheduled jobs only run while the local workspace is unlocked. Each scheduler tick also retries due pending retry queue items with backoff, including account refresh, temp-mail refresh, and backup retries.
- Manual and scheduled automation jobs append status, counts, duration, and detail into `automation_runs`; the Settings UI can filter by job, trigger, status, and detail text before clearing matching rows. Retry jobs are recorded as `retry`.
- Projects synchronize all-account, group, or tag account scope into `project_accounts` and record claim/result events in `project_account_events`.
- Exports are generated from local SQLite data under the app data `exports` directory. Mail HTML export escapes message content rather than executing raw message HTML. Local mail shares reuse the same HTML export path, store revocable records in `email_share_links`, and mark active shares revoked when export files are cleared.

## Build dependencies

- Rust IMAP client depends on `imap-proto`, patched from `vendor/imap-proto-0.10.2` via `[patch.crates-io]` in `src-tauri/Cargo.toml`.

## Next provider work

- Optional local HTTP API remains deferred.
- Gmail OAuth / Gmail API remains deferred unless explicitly reintroduced.
- Gmail, QQ Mail, and 163 Mail provider expansion is tracked in [`provider-integration-plan.md`](provider-integration-plan.md).
- Provider setup, troubleshooting, manual validation, and regression coverage are tracked in [`provider-operations.md`](provider-operations.md).
- Provider expansion should keep the desktop app local-first and reuse SQLite cache, encrypted local secrets, refresh logs, retry queue, attachment cache, and mailbox UI.
- Remaining M9 work is real-account manual validation, not new provider foundation coding.
