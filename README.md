# OutlookEmail Desktop

A modern desktop rebuild of the open-source `assast/outlookEmail` project.

This version is intentionally local-first:

- Tauri 2 desktop shell
- React + Vite + TypeScript UI
- SQLite local database with WAL mode
- Local command bridge instead of a public web API
- No browser extension and no SaaS multi-tenant service

## Current implementation

The implementation includes the desktop project scaffold, SQLite schema, lock/setup flow, account/group/tag management, account tag assignment, local retained-mail workspace, advanced message search/sort/filter/pagination, sandboxed HTML body rendering, batch read/unread/delete actions with remote failure surfacing, local mail/account/project exports, settings storage, Microsoft Graph OAuth, Graph mailbox sync, Graph attachment metadata/download, basic TLS IMAP sync with cached-MIME attachment download, GPTMail/DuckMail/Cloudflare temp-mail management, SMTP/Telegram/WeCom forwarding, failed-operation retry queue, WebDAV backup and local restore, in-app scheduling with unified task history, desktop project account pools, and Windows desktop bundling.

See [`docs/requirements-milestones.md`](docs/requirements-milestones.md) for the full requirement record, completed scope, unfinished scope, and milestone plan.

Gmail, QQ Mail, and 163 Mail provider expansion is tracked in [`docs/provider-integration-plan.md`](docs/provider-integration-plan.md). Current provider setup, troubleshooting, and manual validation steps are documented in [`docs/provider-operations.md`](docs/provider-operations.md). The plan keeps the app desktop-only and does not reintroduce the web service or browser extension.

## Prerequisites

- Node.js 22+
- pnpm
- Rust stable with Cargo
- Platform dependencies required by Tauri 2

## Development

```bash
pnpm install
pnpm tauri:dev
```

If Rust is not installed yet, frontend-only validation can still run:

```bash
pnpm install
pnpm build
pnpm test
```

## Build

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
pnpm tauri:build
```

Successful Windows builds are written to:

- `src-tauri/target/release/outlook-email-desktop.exe`
- `src-tauri/target/release/bundle/msi/OutlookEmail Desktop_0.1.0_x64_en-US.msi`
- `src-tauri/target/release/bundle/nsis/OutlookEmail Desktop_0.1.0_x64-setup.exe`

## Data

The SQLite database is created in the platform application data directory. In development fallback cases, it uses `./outlook-email.sqlite`.

Sensitive fields are encrypted with a key derived from the local app password.

## Mail sync

Graph accounts need a Microsoft client ID and OAuth callback URL. Generate the auth URL in the account authorization panel, paste the callback URL or code back into the app, then refresh the account.

Gmail accounts use Google OAuth and the Gmail API. QQ Mail and 163 Mail use provider-specific IMAP presets with authorization codes or client authorization passwords rather than web login passwords.

IMAP accounts need host, port, and password fields. The IMAP implementation supports TLS password login, caches recent messages in SQLite, stores raw RFC822 MIME for synced messages, and downloads attachments by extracting them from the local cached MIME.

The Mailbox view supports cached-message search, field tokens such as `from:`, `to:`, `subject:`, `body:`, `folder:`, `is:`, and `has:`, read/unread filtering, attachment filtering, multi-field sorting, pagination, sandboxed HTML body rendering, single-message actions, and batch read/unread/delete actions. Graph and IMAP message actions update the local SQLite cache and attempt remote synchronization. Failed remote mark/delete attempts are surfaced on affected messages and queued for manual or scheduled retry; failed deletes keep the cached message visible until the remote delete retry succeeds.

## Local exports

The app can export selected cached messages as a local read-only HTML file. It also exports account inventory and project account pools as CSV files. Exported files are written under the platform app data `exports` directory.

## Forwarding, backup, and scheduling

Forwarding is enabled per account in the account authorization panel. The Settings view configures SMTP, Telegram, and WeCom channels, WebDAV backup credentials, and local scheduler intervals.

The scheduler runs inside the desktop process after the workspace is unlocked. It can periodically refresh mail, forward cached messages, retry failed mailbox refreshes, remote actions, temp-mail refreshes, and backups, and upload a consistent SQLite snapshot created with `VACUUM INTO`. Successful local backup snapshots can be restored from Settings after SQLite integrity validation and a pre-restore safety snapshot. Manual and scheduled refresh/forwarding/backup/retry jobs are recorded in a unified automation history table shown in Settings, where runs can be filtered and cleared.

## Temp mail

The Temp Mail view manages GPTMail, DuckMail, and Cloudflare temporary addresses. It supports address generation, import, message refresh into SQLite, local message browsing, deletion, Cloudflare channel configuration, and refresh-failure retry through the shared retry queue.

## Project account pools

Projects can sync all active accounts, accounts from selected groups, or accounts with selected tags into a local pool. Use the Projects view to claim an account, release it, mark it successful or failed, remove it from the pool, or restore it.
