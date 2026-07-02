# Architecture

## Product boundary

This desktop build keeps OutlookEmail's local mailbox-management workflow and removes the old Flask web server and browser extension.

The application is a single-user local desktop app. It does not expose a public HTTP API by default.

## Runtime

- React/Vite renders the desktop UI.
- Tauri commands are the boundary between UI and native services.
- SQLite stores configuration, accounts, cached mail metadata/body content, temp-mail metadata, forwarding logs, project state, share exports, and audit logs.
- Secrets are encrypted with AES-GCM using a key derived from the local app password.

## Implemented modules

- Local setup/unlock/lock flow
- SQLite schema and default data
- Account import with legacy delimiter support
- Group and tag management
- Cached message workspace
- Settings persistence
- Microsoft Graph OAuth and mailbox refresh
- Basic TLS IMAP mailbox refresh
- Cached message search, filters, pagination, and batch read/unread/delete actions
- GPTMail, DuckMail, and Cloudflare temp-mail management
- SMTP, Telegram, and WeCom forwarding for cached messages
- WebDAV backup from a consistent SQLite snapshot
- Background scheduler inside the desktop process
- Desktop project account pools
- Windows executable, MSI, and NSIS bundle generation

## Provider behavior

- Graph uses Microsoft OAuth v2 and Graph `/me/mailFolders/{folder}/messages` calls.
- Graph attachments are listed during sync and downloaded to the local app data `attachments` directory on demand.
- Graph message read/unread and delete actions update the local cache and attempt Microsoft Graph `PATCH`/`DELETE` synchronization.
- IMAP uses TLS login, UID search/fetch, and MIME parsing.
- IMAP message read/unread and delete actions use UID flag updates; delete applies `\Deleted` and expunges the selected mailbox.
- Temp-mail providers use configurable GPTMail and DuckMail HTTP APIs plus Cloudflare Worker admin channels.
- Temp-mail messages are normalized and cached in `temp_email_messages` for local browsing.
- Refreshed messages are upserted into SQLite and read by the workspace UI.
- Refresh failures are recorded on the account and in `refresh_logs`.
- Forwarding is controlled by a per-account `forward_enabled` flag and deduplicated through `forwarding_logs`.
- Backups are created with SQLite `VACUUM INTO`, stored locally under the app data backup directory, then uploaded with WebDAV `PUT`.
- Scheduled jobs only run while the local workspace is unlocked.
- Projects synchronize account scope into `project_accounts` and record claim/result events in `project_account_events`.

## Next provider work

- IMAP attachment extraction/download from cached raw MIME
- IMAP XOAUTH2 login
- More provider-specific folder discovery
- Safer HTML body rendering policy for cached messages
- Cloudflare AI username generation and advanced batch generation
- Local HTTP API only if external scripts need project-pool access later
