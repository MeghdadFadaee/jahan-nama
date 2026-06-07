# Jahan Nama

Jahan Nama is a small Rust desktop/CLI client for monitoring remaining internet traffic from the Jahan Nama/Webotel ISP API.

It logs in with your account credentials, stores the returned token in your `.env` file, and reuses that token on later runs. The API does not provide refresh tokens, so if the saved token stops working the app logs in again and stores a new token.

## Features

- Windows floating overlay for remaining traffic.
- Tray menu with reload, token reset, settings, and quit actions.
- CLI commands for raw API output, JSON output, diagnostics, and remaining traffic.
- Windows release packaging through GitHub Actions, WiX MSI, Inno Setup installer, and portable zip.

## Configuration

Create a `.env` file next to the executable or in the project root:

```env
JAHAN_NAMA_USERNAME=
JAHAN_NAMA_PASSWORD=
JAHAN_NAMA_TOKEN=
JAHAN_NAMA_INTERVAL_SECONDS=60
```

Only `JAHAN_NAMA_USERNAME` and `JAHAN_NAMA_PASSWORD` are required for the first login. `JAHAN_NAMA_TOKEN` is managed by the app.

Optional GUI settings are saved automatically by the settings window:

```env
JAHAN_NAMA_LABEL_FONT_FAMILY=IRANSansWeb
JAHAN_NAMA_LABEL_FONT_SIZE=14
```

## Usage

Run the Windows overlay:

```powershell
jahan-nama
```

Print remaining traffic:

```powershell
jahan-nama remain
```

Print JSON summary:

```powershell
jahan-nama json
```

Print the raw API response:

```powershell
jahan-nama raw
```

Run the diagnostic flow:

```powershell
jahan-nama test
```

Use a custom env file or polling interval:

```powershell
jahan-nama --env C:\path\to\.env --interval 60
```

`unused` is kept as an alias for `remain`.

## Development

Build:

```powershell
cargo build
```

Run tests:

```powershell
cargo test
```

Build a release binary:

```powershell
cargo build --release
```

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

## API

The app uses the hardcoded Webotel Qom endpoints:

- `POST https://qomservice.webotel.ir/api/login/AuthenticateWeb`
- `GET https://qomservice.webotel.ir/api/BaseInfo/GetUserRemain`

`RemainTraffic` is read from the API response in MB.
