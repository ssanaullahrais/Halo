# Releasing Halo

The repository is `ssanaullahrais/Halo`. The first release is `v0.1.0`.

## Signing

Halo has a dedicated Tauri update-signing key. Its private key must stay outside this repository. Store an encrypted backup somewhere you control; losing it prevents existing installations from trusting future updates.

Configure GitHub Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY`: the private key file's content.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: its password, if one was set.

The matching public key is embedded in `src-tauri/tauri.conf.json`. Update signatures are separate from Windows Authenticode publisher certificates. Do not claim installers are Authenticode-signed unless verified.

## Version and build

1. Update `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json` together.
2. Add release notes to `CHANGELOG.md`.
3. Run `node scripts/check-version.mjs`, `npm run build` and `cargo test --locked --manifest-path src-tauri/Cargo.toml`.
4. Commit, push and create a matching `vX.Y.Z` tag.
5. The Release workflow creates a **draft** with setup EXE, MSI, signatures and `latest.json`.
6. Inspect assets and test a clean install and an update from the previous release before publishing the draft.

Only published stable releases become the automatic update feed. Never publish an empty or incomplete release. The updater endpoint is `https://github.com/ssanaullahrais/Halo/releases/latest/download/latest.json`.

## Runtime behavior

Release builds check after startup and every four hours; development builds do not check automatically. About provides Check for updates. Verified downloads stay in memory until Update & restart is chosen. Active recording/export/conversion/upload work blocks installation. Windows' updater installer exits and relaunches the app as part of updating.

## Windows integration

The standard file context menu is configurable in Settings. Experimental Windows 11 first-menu sparse-package registration has separate developer packaging under `packaging/`; it is not silently installed by the public setup. Do not ship local developer certificates or generated MSIX packages.
