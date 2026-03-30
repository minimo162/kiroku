# Kiroku

Kiroku is a desktop app for capturing work activity on Windows and turning it into structured records. The stack is Tauri v2, Svelte 5, TypeScript, Tailwind CSS, and Rust.

## Development

```bash
npm install
npm run tauri:dev
```

## Build

```bash
npm run check
npm run build
npm run tauri:build
```

If you are building the Windows installer locally, place
`llama-server-x86_64-pc-windows-msvc.exe` under [src-tauri/binaries](src-tauri/binaries/README.md)
before running `npm run tauri:build`. CI can download the sidecar automatically when
the repository variables `LLAMA_SERVER_SIDECAR_URL` and `LLAMA_SERVER_SIDECAR_SHA256`
are configured.

Trusted Signing is supported in CI. To enable MSI code signing, configure these GitHub
secrets and variables:

- Secrets: `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`
- Variables: `TRUSTED_SIGNING_ENDPOINT`, `TRUSTED_SIGNING_ACCOUNT_NAME`, `TRUSTED_SIGNING_CERTIFICATE_PROFILE_NAME`

When they are present, the release workflow signs the generated MSI and runs
`signtool verify /pa /v` against the artifact.

## Docs

The end-user operating guide is available at
[docs/user-manual.md](docs/user-manual.md).
