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
- Account import with legacy delimiter support
- Group and tag management with account tag assignment
- Cached message workspace
- Settings persistence
- Local appearance themes with preset skins and an accent color
- Microsoft Graph OAuth and mailbox refresh
- Basic TLS IMAP mailbox refresh with cached-MIME attachment download
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
- Unified automation run history with filtering and clearing for manual and scheduled refresh/forwarding/backup jobs
- Desktop project account pools
- Windows executable, MSI, and NSIS bundle generation

## Provider behavior

- Account providers are normalized through a shared provider registry. Current ids are `graph`/Outlook, `gmail`, `qq`, `netease_163`, `imap`, and `imap_custom`; `imap` is retained for the existing Outlook IMAP OAuth/generic IMAP path, while `imap_custom` is the new unknown-domain import fallback.
- Graph uses Microsoft OAuth v2 and Graph `/me/mailFolders/{folder}/messages` calls.
- Gmail OAuth uses the Google installed-app flow with PKCE S256 and the `https://www.googleapis.com/auth/gmail.modify` scope so the desktop app can read messages, mark read/unread, and move messages to trash without requesting permanent-delete `https://mail.google.com/` access. The resulting refresh token is stored through the existing encrypted OAuth account fields with provider/account_type normalized to `gmail`.
- Gmail read sync refreshes Google access tokens from the encrypted refresh token, maps `INBOX`/`SPAM`/`TRASH` to the existing inbox/junkemail/deleteditems folders, lists messages with Gmail API `messages.list`, loads details with `messages.get(format=full)`, and stores normalized headers, unread state, snippet, body, and attachment metadata in the same SQLite cache as Graph/IMAP messages.
- Gmail attachment downloads use `users.messages.attachments.get` and reuse the cached Gmail message id plus attachment id from `attachments_json`.
- Gmail incremental sync stores `gmail_history_id` in the account `provider_sync_state` JSON. All-folder refreshes use `users.history.list(startHistoryId=...)`; if Gmail returns HTTP 404 for an expired history id, the adapter falls back to full sync. Changed Gmail message ids are removed from the local cache before current label state is re-upserted so Inbox/Spam/Trash moves do not leave stale folder rows.
- Graph attachments are listed during sync and downloaded to the local app data `attachments` directory on demand.
- Graph message read/unread and delete actions update the local cache and attempt Microsoft Graph `PATCH`/`DELETE` synchronization. Failed remote actions are stored in `retry_queue` with the original account, folder, provider message id, action, error, attempt count, and next attempt time. Failed deletes keep the cached message visible as a rollback surface until the delete retry succeeds.
- IMAP uses TLS login, UID search/fetch, and MIME parsing.
- IMAP folder discovery uses special-use flags where available and falls back to common English and Chinese mailbox names for inbox/junkemail/deleteditems, including QQ/163-style authorization-code accounts.
- IMAP sync stores the raw RFC822 MIME for cached messages so attachment downloads can be resolved locally from SQLite-backed cache data without another network fetch.
- IMAP message read/unread and delete actions use UID flag updates; delete applies `\Deleted` and expunges the selected mailbox. Failed IMAP flag/delete operations use the same retry queue.
- Temp-mail providers use configurable GPTMail and DuckMail HTTP APIs plus Cloudflare Worker admin channels.
- Temp-mail messages are normalized and cached in `temp_email_messages` for local browsing. GPTMail, DuckMail, and Cloudflare refresh failures update the temp mailbox status and queue a `temp_refresh` retry item.
- HTML message bodies are sanitized for active content and rendered in a no-script sandboxed iframe with a restrictive content security policy instead of being inserted into the main React DOM.
- Mailbox search runs against the local SQLite cache and supports free text plus field tokens for sender, recipients, subject, body, folder, read state, attachments, and provider message id. Sorting is validated server-side before being applied to SQL.
- Cached message rows are joined with the latest pending or failed same-folder `mail_mark` / `mail_delete` retry item so the UI can show remote-sync failures and expose retry or dismiss actions without duplicating failure state.
- Refreshed messages are upserted into SQLite and read by the workspace UI.
- Refresh failures are recorded on the account and in `refresh_logs`; failed account refreshes queue a `refresh_account` retry item with account, folder, and page size.
- Provider-aware routing now selects Graph, IMAP, or Gmail before refresh, attachment download, remote read/unread, and delete actions. Gmail delete uses `users.messages.trash`; permanent delete is intentionally not exposed.
- Forwarding is controlled by a per-account `forward_enabled` flag and deduplicated through `forwarding_logs`. Failed SMTP/Telegram/WeCom sends are queued with message id and channel for later replay.
- Backups are created with SQLite `VACUUM INTO`, stored locally under the app data backup directory, then uploaded with WebDAV `PUT`. Failed backup attempts queue a `backup_job` retry item.
- Restores are limited to successful backup log entries that resolve to local `.sqlite` snapshots under the app backup directory. The restore command validates the snapshot with SQLite `integrity_check`, creates a pre-restore safety snapshot, replaces the current database file set, reopens SQLite, and audits the restore.
- Scheduled jobs only run while the local workspace is unlocked. Each scheduler tick also retries due pending retry queue items with backoff, including account refresh, temp-mail refresh, and backup retries.
- Manual and scheduled automation jobs append status, counts, duration, and detail into `automation_runs`; the Settings UI can filter by job, trigger, status, and detail text before clearing matching rows. Retry jobs are recorded as `retry`.
- Projects synchronize all-account, group, or tag account scope into `project_accounts` and record claim/result events in `project_account_events`.
- Exports are generated from local SQLite data under the app data `exports` directory. Mail HTML export escapes message content rather than executing raw message HTML. Local mail shares reuse the same HTML export path, store revocable records in `email_share_links`, and mark active shares revoked when export files are cleared.

## Next provider work

- Optional local HTTP API remains deferred.
- Gmail, QQ Mail, and 163 Mail provider expansion is tracked in [`provider-integration-plan.md`](provider-integration-plan.md).
- Provider setup, troubleshooting, manual validation, and regression coverage are tracked in [`provider-operations.md`](provider-operations.md).
- Provider expansion should keep the desktop app local-first and reuse SQLite cache, encrypted local secrets, refresh logs, retry queue, attachment cache, and mailbox UI.
- Provider foundation work has started with registry-backed import detection, provider labels and badges, provider capability metadata, provider filtering, batch import provider/credential previews, refresh failure provider summaries, IMAP presets for QQ/163, provider-specific credential hints, centralized adapter routing, Gmail OAuth account save support, and Gmail API first-read sync.
- Gmail should be implemented as a first-class OAuth/API provider first, with IMAP XOAUTH2 only as a fallback path.
- QQ Mail and 163 Mail should initially be provider presets over the generic IMAP adapter, using provider-specific setup hints, import auto-detection, folder mapping validation, and real-account verification.
- Remaining foundation work should continue replacing hard-coded Outlook/Graph labels where they are tied to OAuth setup rather than general account display.
