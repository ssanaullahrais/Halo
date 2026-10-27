# Halo in the Windows 11 main right-click menu

`shellext/` is a Rust cdylib (`halo_menu.dll`, `windows` crate, `#[implement]`) exposing an `IExplorerCommand`
"Halo" submenu (flat shortcuts filtered by the shared rules in `shared/capabilities.rs`).
`Invoke` runs `halo.exe --action <verb[:option[:value]]> "<path>"` once per selected file, from the folder the DLL is in.
`AppxManifest.xml` is a sparse MSIX (`uap10:AllowExternalContent`) declaring `windows.fileExplorerContextMenus`
for `*` and `Directory`; the external location is the folder holding `halo.exe` and `halo_menu.dll`.

```powershell
.\packaging\build-sparse.ps1      # build DLL, copy next to halo.exe, create test cert, pack + sign packaging\out\HaloContextMenu.msix
.\packaging\install-sparse.ps1    # trust cert (one UAC prompt), Add-AppxPackage -ExternalLocation, turn classic entries off
.\packaging\uninstall-sparse.ps1  # remove package + cert, turn classic entries back on
```

Both default to `src-tauri\target\release` if `halo.exe` is there, else `target\debug`; override with `-InstallDir`.
After installing, restart Explorer once (`Stop-Process -Name explorer -Force`, closes open folder windows) if Halo is missing.

## No duplicate menus
While the package is registered, `sparse_installed()` in `main.rs` keeps the classic registry entries off
(the home-window switch shows On, and turning it Off removes the package). Uninstalling the package restores classic.

## Production
A self-signed certificate only works because it is trusted on this PC. For distribution:
1. Sign with a real code-signing certificate (or Azure Trusted Signing): `build-sparse.ps1 -CertThumbprint <thumb>`.
2. Set `Publisher` in `AppxManifest.xml` to that certificate's exact subject.
3. Ship `halo_menu.dll` next to `halo.exe` in the installer, and have the installer run
   `Add-AppxPackage -Path HaloContextMenu.msix -ExternalLocation <install dir>` (no cert import needed with a trusted CA).

## Test
`cd shellext; cargo build --release; cargo test --release -- --nocapture --test-threads=1`
(`HALO_DLL`, `HALO_TEST_IMAGE`, `HALO_TEST_REGISTERED=1` and `HALO_TEST_INVOKE=<shortcut title>` tune it; see `tests/com.rs`).

## Local standalone integration and hot reload

This machine registers the sparse package against `output/windows-integration`. That directory contains
a standalone `halo.exe` with the frontend embedded, the COM DLL, the signed identity package, and
`resources/pdfium.dll`. Explorer commands therefore work with Vite stopped.

The standalone copy was built with `npm run tauri build -- --debug --no-bundle`. It is a local development
build, not a release installer. Rebuild and copy it after app changes if Explorer needs the latest code
while the hot-reload app is closed.

`output/run-halo-dev.ps1` runs hot reload and sets `HALO_NATIVE_MENU_DIR` to the standalone directory, so
the settings switch also registers that copy. Both copies use the same single-instance identifier;
Explorer hands actions to the development app when it is already running.

To test registered COM activation and invocation, set `HALO_TEST_REGISTERED=1`, `HALO_DLL` to the installed
DLL, `HALO_TEST_IMAGE` to a disposable fixture, and `HALO_TEST_INVOKE` to an exact shortcut title such as
`Zip it`, then run the test command above.
