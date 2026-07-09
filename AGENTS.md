# OutlookEmail Desktop — Agent Entry

Local-first desktop rebuild of `assast/outlookEmail`. **No web service, no browser extension, no SaaS.**

## Current milestone

- **M0–M8:** Done.
- **M9:** Code complete — Gmail/QQ/163 IMAP presets, provider registry, UX, docs, readiness/failure hints. **Pending real-account manual QA only.**

## Blockers

- Real Gmail / QQ / 163 test accounts for manual acceptance per `docs/provider-operations.md`.

## Stack

| Layer | Tech |
|-------|------|
| Shell | Tauri 2 |
| UI | React 19 + Vite + TypeScript |
| Backend | Rust Tauri commands (`src-tauri/src/`) |
| DB | SQLite WAL — dev fallback `./outlook-email.sqlite` |
| Secrets | AES-GCM from local app password |
| IMAP | `imap-proto` vendored at `vendor/imap-proto-0.10.2` |

**Providers:** `graph` (Microsoft OAuth), `gmail`/`qq`/`netease_163` (IMAP presets), `imap` (Outlook IMAP OAuth XOAUTH2), `imap_custom`.

**OAuth scope:** Microsoft only (`graph`, Outlook `imap`). Gmail OAuth/API **disabled in code** — Gmail uses IMAP app password (`imap.gmail.com:993`).

## Key paths

```
src/                    # React UI, api.ts, lib/*
  lib/providerRegistry.ts   # Frontend provider metadata + readiness/failure hints
  lib/importParser.ts       # Account import with provider detection
  lib/emailHtml.ts          # Sanitized sandbox iframe rendering
  lib/mailPreview.ts        # List/preview snippet HTML/CSS stripping
src-tauri/src/
  commands.rs           # Tauri command surface
  db.rs                 # SQLite + business logic
  providers.rs          # Provider registry + Graph/IMAP adapters
  import.rs             # Account import
  scheduler.rs          # Background mail refresh scheduler
docs/
  requirements-milestones.md   # Scope, milestones, done/undone
  provider-integration-plan.md # M9 plan (Gmail/QQ/163 IMAP)
  provider-operations.md       # Troubleshooting + manual QA checklist
  architecture.md              # Runtime + module map
```

**Main UI views:** mail, accounts, settings.

## Commands

```bash
pnpm install
pnpm tauri:dev          # full desktop dev
pnpm build && pnpm test # frontend-only (no Rust)
pnpm cargo:test       # Rust lib tests (uses E:\RustCache target dirs)
pnpm tauri:build      # Windows exe / MSI / NSIS
```

## Conventions

- Local-first: new features via Tauri commands + SQLite, not HTTP API.
- Provider changes: update **both** `src/lib/providerRegistry.ts` and `src-tauri/src/providers.rs`.
- Mail HTML: sanitize + sandboxed iframe (`emailHtml.ts`); list snippets via `mailPreview.ts`.
- Failed remote mark/delete actions surface in job results; local cache updates immediately.
- Tests: `pnpm test` + `pnpm cargo:test` before provider-related merges.

## Docs to open by task

| Task | Read first |
|------|------------|
| Scope / what's done | `docs/requirements-milestones.md` |
| Gmail/QQ/163 work | `docs/provider-integration-plan.md` |
| Manual QA / errors | `docs/provider-operations.md` |
| Architecture | `docs/architecture.md` |

## Deferred (do not implement unless asked)

Web service, browser extension, Docker deploy, public HTTP API, Gmail OAuth/Gmail API, temp mail, forwarding, WebDAV backup, retry queue, automation dashboard, project pools.

## Session bootstrap

New agents: read this file + **当前推荐下一步** in `docs/requirements-milestones.md`. Do **not** re-read the full codebase unless the task requires it. Docs were synced to code on **2026-07-09**.
