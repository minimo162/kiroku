Place the Windows sidecar binary here before running `npm run tauri build`.

Expected filename:

- `llama-server-x86_64-pc-windows-msvc.exe`

The release workflow can also download it when the repository variable
`LLAMA_SERVER_SIDECAR_URL` points to either:

- a direct `llama-server.exe` URL
- a ZIP asset URL that contains `llama-server.exe`

For reproducible CI builds, also set:

- `LLAMA_SERVER_SIDECAR_SHA256`: expected SHA256 of the downloaded executable
