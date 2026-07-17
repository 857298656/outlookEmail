# Windows Release

The GitHub Actions release workflow builds the NSIS installer, signs its updater artifact, and uploads `latest.json`.

## Required repository secrets

Configure both values under **Settings > Secrets and variables > Actions**:

- `TAURI_SIGNING_PRIVATE_KEY`: the complete contents of `%USERPROFILE%\.tauri\outlookemail.key`.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the password used when that updater signing key was generated.

Never commit the private key or its password. The matching public key is stored in `src-tauri/tauri.conf.json` and may be committed.

The local public key file `%USERPROFILE%\.tauri\outlookemail.key.pub` must match `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`. If the signing key is replaced, update the configured public key at the same time; otherwise installed clients will reject new updates.

## Publishing

1. Run `pnpm build`, `pnpm test`, and `pnpm cargo:test`.
2. Update the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
3. Push a matching `v*` tag.
4. Confirm that the release contains the NSIS installer, its `.sig` file, and `latest.json`.

The workflow validates signing secrets before the expensive Windows bundle step so a missing secret fails with a direct diagnostic.
