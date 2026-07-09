# OutlookEmail Desktop

A modern desktop rebuild of the open-source `assast/outlookEmail` project.

This version is intentionally local-first:

- Tauri 2 desktop shell
- React + Vite + TypeScript UI
- SQLite local database with WAL mode
- Local command bridge instead of a public web API
- No browser extension and no SaaS multi-tenant service

## Current implementation

The implementation includes the desktop project scaffold, SQLite schema, lock/setup flow, account/group/tag/alias management, provider registry with Gmail/QQ/163 IMAP presets, provider readiness checks and failure hints, account tag assignment and batch operations, local retained-mail workspace with full-screen preview dialog and cleaned list snippets, advanced message search/sort/filter/pagination, sandboxed HTML body rendering, batch read/unread/delete actions with remote failure surfacing in job results, local mail share records, local HTML/CSV exports and confirmed account-secret export, settings storage with local retention cleanup, scheduled mail refresh, Microsoft Graph OAuth (Outlook only), Graph mailbox sync, Graph attachment metadata/download, TLS IMAP sync with cached-MIME attachment download and NetEase IMAP `ID` support, and Windows desktop bundling.

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

Graph accounts need a Microsoft client ID and OAuth callback URL. Generate the auth URL in the account authorization panel, paste the callback URL or code back into the app, then refresh the account. OAuth is limited to Outlook Graph and Outlook IMAP providers; attempting Gmail OAuth returns an error.

Gmail accounts currently use IMAP with `imap.gmail.com:993` and a Google account app password; Google OAuth login is not used for Gmail. QQ Mail and 163 Mail use provider-specific IMAP presets with authorization codes or client authorization passwords rather than web login passwords.

The account inventory shows provider-aware readiness details before refresh, including missing OAuth Client IDs, refresh tokens, IMAP host/port values, and provider-specific IMAP authorization secrets.

IMAP accounts need host, port, and password fields. The IMAP implementation supports TLS password login, caches recent messages in SQLite, stores raw RFC822 MIME for synced messages, and downloads attachments by extracting them from the local cached MIME.

The Mailbox view supports cached-message search, field tokens such as `from:`, `to:`, `subject:`, `body:`, `folder:`, `is:`, and `has:`, read/unread filtering, attachment filtering, multi-field sorting, pagination, cleaned list preview text, a full-screen preview dialog with share/export/raw actions, sandboxed HTML body rendering, single-message actions, and batch read/unread/delete actions. Graph and IMAP message actions update the local SQLite cache and attempt remote synchronization. Failed remote mark/delete attempts are reported in job results.

## Local exports

The app can export selected cached messages as a local read-only HTML file. It also exports account inventory as CSV. Exported files are written under the platform app data `exports` directory.

## Scheduling

The scheduler runs inside the desktop process after the workspace is unlocked. Settings can enable periodic mail refresh with a configurable interval and message top count. Manual refresh is available from the mailbox and account views.
