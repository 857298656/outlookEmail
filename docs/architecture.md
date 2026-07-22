# Architecture

## Product boundary

This desktop build keeps OutlookEmail's local mailbox-management workflow and removes the old Flask web server and browser extension.

The application is a single-user local desktop app. It does not expose a public HTTP API by default.

## Runtime

- React/Vite renders the desktop UI.
- Tauri commands are the boundary between UI and native services.
- SQLite stores configuration, accounts, temporary mailbox metadata, cached mail metadata/body content, Markdown documents/categories, share exports, workspace keys, and audit logs.
- Secrets are encrypted with AES-GCM using a key derived from the local app password.

## Implemented modules

- Local setup/unlock/lock flow
- SQLite schema and default data
- Account import with legacy delimiter support and provider-aware auto-detection
- Group and tag management with account tag assignment
- Cached message workspace with full-screen mail preview dialog
- Mail list preview text formatting (`src/lib/mailPreview.ts`) that strips HTML/CSS noise from snippets
- Settings persistence, local retention cleanup, and scheduled mail refresh
- Microsoft Graph OAuth and mailbox refresh (OAuth limited to `graph` and Outlook `imap` providers)
- Generic TLS IMAP mailbox refresh for Gmail/QQ/163/Custom IMAP and Outlook IMAP OAuth (XOAUTH2)
- Cached message field-token search, filters, multi-field sorting, pagination, batch read/unread/delete actions, and remote failure surfacing in job results
- Sandboxed HTML body rendering for cached mailbox messages, with sanitized HTTP(S) links delegated to the system default browser
- Local HTML/CSV exports for cached mail and account inventory
- Confirmed CSV export for selected account secrets after local password verification
- Local mail share records with generated HTML files, expiration, and revocation
- Background scheduler inside the desktop process (mail refresh only)
- Provider registry, readiness checks, failure hints, and credential error classification
- Workspace key records for local operational secrets
- Markdown workspace with SQLite-backed documents/nested folders, title/body filtering, revision-safe debounced autosave, a Milkdown Crepe WYSIWYG editor, embedded pasted/dropped images, native `.md` / `.markdown` open/link/save flows, `.md` / `.markdown` / `.txt` / `.json` SQLite-copy imports, and Markdown folder exports; the workspace shell and Crepe runtime are loaded as separate lazy frontend chunks
- GPTMail, DuckMail, and Cloudflare Temp Email management through native Tauri commands, including chunked provider-aware batch import with visible progress, Cloudflare batch generation (1–50 addresses with partial-failure results), and SQLite message caching; provider credentials remain encrypted locally and received HTML uses the existing sandbox renderer
- Windows executable/NSIS bundle generation and universal macOS DMG generation for Intel and Apple Silicon

## Provider behavior

- Account providers are normalized through a shared provider registry. Current ids are `graph`/Outlook, `gmail`, `qq`, `netease_163`, `imap`, and `imap_custom`; `imap` is retained for the existing Outlook IMAP OAuth/generic IMAP path, while `imap_custom` is the new unknown-domain import fallback.
- Graph uses Microsoft OAuth v2 and Graph `/me/mailFolders/{folder}/messages` calls. OAuth auth URL generation and token exchange accept `graph` and Outlook `imap` providers only.
- Gmail uses the generic TLS IMAP adapter with preset `imap.gmail.com:993` and a Google account app password stored as the encrypted IMAP password. Backend rejects Gmail/Google OAuth requests with an explicit error.
- Gmail attachment downloads use the same IMAP raw-MIME cache path as other IMAP providers.
- Graph attachments are listed during sync and downloaded to the local app data `attachments` directory on demand.
- Graph message read/unread and delete actions update the local cache and attempt Microsoft Graph `PATCH`/`DELETE` synchronization. Failed remote actions are reported in job results; the local cache is updated immediately.
- IMAP uses TLS login, UID search/fetch, and MIME parsing. Outlook IMAP OAuth accounts authenticate with XOAUTH2 using Microsoft refresh tokens.
- IMAP folder discovery uses special-use flags where available and falls back to common English and Chinese mailbox names for inbox/junkemail/deleteditems, including `[Gmail]/Spam`, QQ/163-style folders, and Chinese names such as `垃圾邮件` / `已删除邮件` / `回收站`.
- NetEase IMAP accounts (`netease_163`, `imap.163.com`, `imap.126.com`, `imap.yeah.net`) send an IMAP `ID` command after login with a fixed client identity string.
- IMAP sync stores the raw RFC822 MIME for cached messages so attachment downloads can be resolved locally from SQLite-backed cache data without another network fetch.
- IMAP message read/unread and delete actions use UID flag updates; delete applies `\Deleted` and expunges the selected mailbox. Failed IMAP flag/delete operations are reported in job results.
- HTML message bodies are sanitized for active content and rendered in a no-script sandboxed iframe with a restrictive content security policy instead of being inserted into the main React DOM.
- Temporary mail is provider-backed rather than a local SMTP receiver. GPTMail uses its API key endpoints; DuckMail creates or imports an account, stores its password/token encrypted, discovers verified domains, and reads messages through its bearer-token API. Cloudflare channels store a Worker URL, encrypted administrator password, enabled state, and allowed domains; address creation, batch generation, import, and deletion use the Worker administrator endpoints, while received raw MIME from `/admin/mails` is parsed locally before rendering. Batch import accepts one GPTMail address per line, `address----password` for DuckMail, and Cloudflare channel sections such as `[cloudflare:channel name]`; the UI submits fixed-size chunks and displays chunk progress. Message lists and fetched bodies are upserted into SQLite before being shown; cached messages remain readable when a later provider refresh fails.
- Mail list rows and preview headers use `formatMailPreview` / `formatMessageListPreview` to strip tags, inline CSS blocks, and HTML entities from snippet text before display.
- The mailbox detail experience is a modal preview dialog with read/unread actions, raw MIME view, share/export shortcuts, and active share records.
- Mailbox search runs against the local SQLite cache and supports free text plus field tokens for sender, recipients, subject, body, folder, read state, attachments, and provider message id. Sorting is validated server-side before being applied to SQL.
- Refreshed messages are upserted into SQLite and read by the workspace UI.
- Refresh failures are recorded on the account. Provider-aware routing selects Graph or IMAP before refresh, attachment download, remote read/unread, and delete actions. Gmail routes to IMAP.
- Provider readiness shown in the account inventory is derived from non-secret account flags such as Client ID presence, refresh token presence, IMAP host/port, and IMAP password presence; it does not decrypt or display stored secrets.
- Exports are generated from local SQLite data under the app data `exports` directory. Mail HTML export escapes message content rather than executing raw message HTML. Local mail shares reuse the same HTML export path, store revocable records in `email_share_links`, and mark active shares revoked when export files are cleared.

## Scheduler

- `scheduler.rs` runs inside the desktop process after the workspace is unlocked.
- The scheduler currently supports periodic mail refresh only (`scheduler_refresh_enabled`, interval, and top count in Settings).
- Manual refresh is available from the mailbox and account views via `run_refresh_job`.

## Schema maintenance

- On startup, `Database::initialize_schema()` creates the current tables, including nested `markdown_categories` and `markdown_documents`, applies additive Markdown column migration, and then runs `prune_legacy_schema()`.
- That migration preserves the current `temp_emails` and `temp_email_messages` tables while dropping obsolete forwarding/backup logs, automation history, retry queue, and project-pool tables. It also deletes obsolete `app_config` keys and unused `accounts` columns such as `forward_enabled`.
- The prune step is idempotent and safe to rerun on every launch.

## Build dependencies

- Rust IMAP client depends on `imap-proto`, patched from `vendor/imap-proto-0.10.2` via `[patch.crates-io]` in `src-tauri/Cargo.toml`.

## Next provider work

- Optional local HTTP API remains deferred.
- Gmail OAuth / Gmail API remains deferred unless explicitly reintroduced.
- Gmail, QQ Mail, and 163 Mail provider expansion is tracked in [`provider-integration-plan.md`](provider-integration-plan.md).
- Provider setup, troubleshooting, manual validation, and regression coverage are tracked in [`provider-operations.md`](provider-operations.md).
- Provider expansion should keep the desktop app local-first and reuse SQLite cache, encrypted local secrets, attachment cache, and mailbox UI.
- Remaining M9 work is real-account manual validation, not new provider foundation coding.
