# OutlookEmail Desktop

A modern desktop rebuild of the open-source `assast/outlookEmail` project.

This version is intentionally local-first:

- Tauri 2 desktop shell
- React + Vite + TypeScript UI
- SQLite local database with WAL mode
- Local command bridge instead of a public web API
- No browser extension and no SaaS multi-tenant service

## Current implementation

The implementation includes the desktop project scaffold, SQLite schema, lock/setup flow, account/group/tag management, local retained-mail workspace, settings storage, Microsoft Graph OAuth, Graph mailbox sync, Graph attachment metadata/download, basic TLS IMAP sync, GPTMail/DuckMail/Cloudflare temp-mail management, SMTP/Telegram/WeCom forwarding, WebDAV backup, in-app scheduling, desktop project account pools, and Windows desktop bundling.

See [`docs/requirements-milestones.md`](docs/requirements-milestones.md) for the full requirement record, completed scope, unfinished scope, and milestone plan.

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

IMAP accounts need host, port, and password fields. The first IMAP implementation supports TLS password login and caches recent messages in SQLite. IMAP attachment metadata is parsed from MIME messages; direct IMAP attachment extraction is still pending.

## Forwarding, backup, and scheduling

Forwarding is enabled per account in the account authorization panel. The Settings view configures SMTP, Telegram, and WeCom channels, WebDAV backup credentials, and local scheduler intervals.

The scheduler runs inside the desktop process after the workspace is unlocked. It can periodically refresh mail, forward cached messages, and upload a consistent SQLite snapshot created with `VACUUM INTO`.

## Temp mail

The Temp Mail view manages GPTMail, DuckMail, and Cloudflare temporary addresses. It supports address generation, import, message refresh into SQLite, local message browsing, deletion, and Cloudflare channel configuration.

## Project account pools

Projects can sync all active accounts or accounts from selected groups into a local pool. Use the Projects view to claim an account, release it, mark it successful or failed, remove it from the pool, or restore it.
