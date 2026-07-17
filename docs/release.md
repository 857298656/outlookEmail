# Windows Release

The GitHub Actions release workflow builds the NSIS installer, signs its updater artifact, and uploads `latest.json`.

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

## Publishing

1. Run `pnpm build`, `pnpm test`, and `pnpm cargo:test`.
2. Update the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
3. Push a matching `v*` tag.
4. Confirm that the release contains the NSIS installer, its `.sig` file, and `latest.json`.

The workflow validates signing secrets before the expensive Windows bundle step so a missing secret fails with a direct diagnostic.
