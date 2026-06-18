# Jahan Nama

Jahan Nama is a small Rust desktop/CLI client for monitoring remaining internet traffic from the Jahan Nama/Webotel ISP API.

It logs in with your account credentials, stores the returned token in your `.env` file, and reuses that token on later runs. The API does not provide refresh tokens, so if the saved token stops working the app logs in again and stores a new token.

## Features

- macOS menu-bar app with a floating overlay, settings window, reload, token reset, and quit actions.
- Windows floating overlay with tray menu, reload, token reset, settings, and quit actions.
- CLI commands for raw API output, JSON output, diagnostics, and remaining traffic.
- Windows release packaging through GitHub Actions, WiX MSI, Inno Setup installer, and portable zip.
- macOS `.app` and unsigned `.dmg` packaging through `scripts/package-macos.sh`.

## Configuration

Create a `.env` file next to the executable or in the project root:

```env
JAHAN_NAMA_USERNAME=
JAHAN_NAMA_PASSWORD=
JAHAN_NAMA_TOKEN=
JAHAN_NAMA_INTERVAL_SECONDS=60
JAHAN_NAMA_LABEL_FONT_FAMILY=SF Pro Text
JAHAN_NAMA_LABEL_FONT_SIZE=15
JAHAN_NAMA_OVERLAY_VISIBLE=true
JAHAN_NAMA_OVERLAY_X=
JAHAN_NAMA_OVERLAY_Y=
```

Only `JAHAN_NAMA_USERNAME` and `JAHAN_NAMA_PASSWORD` are required for the first login. `JAHAN_NAMA_TOKEN` is managed by the app.

Optional GUI settings are saved automatically by the settings window. A Finder-launched macOS app uses `~/Library/Application Support/Jahan Nama/.env` when no custom `--env` path is provided.

## Usage

Run the desktop app:

```sh
jahan-nama
```

Print remaining traffic:

```sh
jahan-nama remain
```

Print JSON summary:

```sh
jahan-nama json
```

Print the raw API response:

```sh
jahan-nama raw
```

Run the diagnostic flow:

```sh
jahan-nama test
```

Use a custom env file or polling interval:

```sh
jahan-nama --env /path/to/.env --interval 60
```

`unused` is kept as an alias for `remain`.

## Development

On macOS, install Rust first:

```sh
brew install rustup
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
rustup default stable
echo 'export PATH="/opt/homebrew/opt/rustup/bin:$PATH"' >> ~/.zshrc
```

Restart Terminal after updating `~/.zshrc`, or run this in the current shell:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
```

Build:

```sh
cargo build
```

Run tests:

```sh
cargo test
```

Build a release binary:

```sh
cargo build --release
```

Build an unsigned macOS DMG:

```sh
bash scripts/package-macos.sh
```

Unsigned macOS apps may need to be opened with right-click `Open` the first time.

The project intentionally does not keep `Cargo.lock` in the repository.

## Release

Push a version tag to run the release workflow:

```powershell
git tag v2.1.0
git push origin v2.1.0
```

The workflow builds:

- `jahan-nama-<version>-windows-x64-portable.zip`
- `jahan-nama-<version>-windows-x64.msi`
- `jahan-nama-<version>-windows-x64-setup.exe`
- `jahan-nama-<version>-windows-x64-setup-bundle.zip`
- `jahan-nama-<version>-macos-arm64.dmg`

## API

The app uses the hardcoded Webotel Qom endpoints:

- `POST https://qomservice.webotel.ir/api/login/AuthenticateWeb`
- `GET https://qomservice.webotel.ir/api/BaseInfo/GetUserRemain`

`RemainTraffic` is read from the API response in MB.
