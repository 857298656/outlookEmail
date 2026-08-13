# Windows and macOS Release

The GitHub Actions release workflow builds the Windows NSIS installer first, then builds a universal macOS DMG for Intel and Apple Silicon. Both updater artifacts are signed with the Tauri updater key and merged into `latest.json`.

## Required repository secrets

Configure both values under **Settings > Secrets and variables > Actions**:

- `TAURI_SIGNING_PRIVATE_KEY`: the complete contents of `%USERPROFILE%\.tauri\outlookemail.key`.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the password used when that updater signing key was generated.

Never commit the private key or its password. The matching public key is stored in `src-tauri/tauri.conf.json` and may be committed.

The local public key file `%USERPROFILE%\.tauri\outlookemail.key.pub` must match `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`. If the signing key is replaced, update the configured public key at the same time; otherwise installed clients will reject new updates.

## Local signed build

GitHub repository secrets are available only inside GitHub Actions. Do not copy the private key or its password into source files, `.env` files, or `tauri.conf.json`.

For a signed local updater build, set both values only in the current PowerShell process:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -LiteralPath "$env:USERPROFILE\.tauri\outlookemail.key" -Raw
$securePassword = Read-Host "Updater signing password" -AsSecureString
$passwordPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($securePassword)
try {
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($passwordPointer)
  pnpm tauri:build
} finally {
  [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordPointer)
  Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
  Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
}
```

The variables exist only for that PowerShell session and are removed after the build.

## macOS build and signing

The macOS package must be built on macOS. The local command and CI workflow both create a universal DMG:

```bash
pnpm tauri:build:mac
```

The generated installer is under `universal-apple-darwin/release/bundle/dmg/`. The platform configuration currently uses the ad-hoc signing identity `-`, so the DMG can be produced without an Apple Developer certificate. Users must approve the first launch in **System Settings > Privacy & Security**.

For warning-free public distribution, replace ad-hoc signing in CI with a Developer ID Application certificate and Apple notarization credentials. Follow the official Tauri macOS signing guide and store every credential as a GitHub Actions secret; never commit certificates, app-specific passwords, API keys, or private keys.

## Publishing

1. Run `pnpm build`, `pnpm test`, and `pnpm cargo:test`.
2. Update the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`, then replace `RELEASE_BODY` in `.github/workflows/release.yml` with the new release notes.
3. Push a matching `v*` tag.
4. Confirm that the release contains the NSIS installer, universal DMG, both updater artifacts and `.sig` files, and a `latest.json` with Windows plus macOS platform entries.

The workflow validates updater signing secrets before each bundle step. The macOS job runs after Windows so `tauri-action` can merge the macOS updater entry into the existing cross-platform `latest.json` without a concurrent upload race.
Both platform jobs must pass the same `releaseBody`. The updater dialog reads `latest.json.notes`, and the final macOS publish step can otherwise replace the release body and generated notes with an empty value.
