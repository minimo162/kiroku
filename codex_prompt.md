# Codex 実装指示: Copilot 接続の完全自動化と競合回避 (T26〜T28)

## 方針

- **T26 → T27 → T28 の順で実装すること**（依存関係あり）
- `cargo check` でエラーがないことを確認してから次のタスクへ進むこと
- 既存テスト（`cargo test`）が壊れないこと
- copilot_server.ts の変更後は `npx esbuild src/copilot_server.ts --bundle --platform=node --outfile=src-tauri/binaries/copilot_server.js --format=esm --external:playwright` でバンドルすること

---

## T26: Kiroku 専用 Edge プロファイルと非標準 CDP ポート

### 目的
他の Copilot 自動化ツールやユーザーの通常 Edge との競合を防ぐ。
`--user-data-dir` で専用プロファイルを使い、CDP ポートを非標準に変更する。

### 対象ファイル
- `src/copilot_server.ts`
- `src-tauri/src/vlm/copilot_server.rs`
- `src-tauri/src/models.rs`
- `src-tauri/src/config.rs`

### 変更内容

#### 1. src/copilot_server.ts

**DEFAULT_CDP_PORT を変更:**
```typescript
// 変更前
const DEFAULT_CDP_PORT = 9222;
// 変更後
const DEFAULT_CDP_PORT = 9333;
```

**ParsedArgs 型に userDataDir を追加:**
```typescript
type ParsedArgs = {
  port: number;
  cdpPort: number;
  userDataDir: string | null;
  help: boolean;
};
```

**parseArgs() に --user-data-dir を追加:**
```typescript
function parseArgs(argv: string[]): ParsedArgs {
  let port = DEFAULT_PORT;
  let cdpPort = DEFAULT_CDP_PORT;
  let userDataDir: string | null = null;
  let help = false;

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") {
      help = true;
      continue;
    }
    if (arg === "--port") {
      port = parsePort(argv[index + 1], "--port");
      index += 1;
      continue;
    }
    if (arg === "--cdp-port") {
      cdpPort = parsePort(argv[index + 1], "--cdp-port");
      index += 1;
      continue;
    }
    if (arg === "--user-data-dir") {
      userDataDir = argv[index + 1] ?? null;
      index += 1;
      continue;
    }
  }

  return { port, cdpPort, userDataDir, help };
}
```

**launchEdgeForCdp() で --user-data-dir を使用:**
```typescript
function launchEdgeForCdp(edgeExecutable: string, cdpPort: number): void {
  const args = [
    `--remote-debugging-port=${cdpPort}`,
    "--remote-allow-origins=*",
    "--no-first-run",
    "--no-default-browser-check",
  ];

  // Kiroku 専用プロファイルで起動（他ツールとの競合を防ぐ）
  if (globalOptions.userDataDir) {
    args.push(`--user-data-dir=${globalOptions.userDataDir}`);
  }

  args.push(COPILOT_URL);

  const child = spawn(edgeExecutable, args, {
    detached: true,
    stdio: "ignore",
    windowsHide: true,
  });
  child.unref();
}
```

**main() の help メッセージを更新:**
```typescript
console.error("Usage: node copilot_server.js [--port 18080] [--cdp-port 9333] [--user-data-dir <path>]");
```

#### 2. src-tauri/src/vlm/copilot_server.rs

**CopilotServer に edge_profile_dir を追加:**
```rust
#[derive(Debug)]
pub struct CopilotServer {
    process: Option<Child>,
    port: u16,
    cdp_port: u16,
    client: Client,
    script_path: Option<PathBuf>,
    edge_profile_dir: PathBuf,
}
```

**new() を変更:**
```rust
pub fn new(config: &AppConfig, app_paths: &AppPaths) -> Result<Self, VlmError> {
    let edge_profile_dir = app_paths.data_dir.join("edge-profile");
    Ok(Self {
        process: None,
        port: config.copilot_port,
        cdp_port: config.edge_cdp_port,
        client: Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(VlmError::Http)?,
        script_path: resolve_script_path(app_paths),
        edge_profile_dir,
    })
}
```

**start() の args に --user-data-dir を追加:**

既存の args 配列:
```rust
.args([
    &script_path.to_string_lossy().into_owned(),
    "--port",
    &self.port.to_string(),
    "--cdp-port",
    &self.cdp_port.to_string(),
])
```

を以下に変更:
```rust
.args({
    let mut args = vec![
        script_path.to_string_lossy().into_owned(),
        "--port".to_string(),
        self.port.to_string(),
        "--cdp-port".to_string(),
        self.cdp_port.to_string(),
        "--user-data-dir".to_string(),
        self.edge_profile_dir.to_string_lossy().into_owned(),
    ];
    args
}.iter().map(|s| s.as_str()).collect::<Vec<_>>())
```

注意: `.args()` は `&[&str]` を受け取るため、`let args_owned: Vec<String>` を作ってから参照を渡す方がクリーン:

```rust
let args_owned = vec![
    script_path.to_string_lossy().into_owned(),
    "--port".to_string(),
    self.port.to_string(),
    "--cdp-port".to_string(),
    self.cdp_port.to_string(),
    "--user-data-dir".to_string(),
    self.edge_profile_dir.to_string_lossy().into_owned(),
];

let child = match Command::new(node)
    .args(&args_owned)
    .stdout(Stdio::null())
    .stderr(Stdio::from(stderr_file))
    .spawn()
```

#### 3. src-tauri/src/models.rs

**edge_cdp_port のデフォルトを変更:**
```rust
// 変更前
edge_cdp_port: 9222,
// 変更後
edge_cdp_port: 9333,
```

2 箇所（Default impl と with_data_dir）の両方を変更すること。

#### 4. src-tauri/src/config.rs

**load_config() にマイグレーションを追加:**

既存のマイグレーション処理（`batch_times.is_empty()` チェック等）の近くに追加:

```rust
// CDP ポートのマイグレーション: 9222 → 9333（競合回避）
if config.edge_cdp_port == 9222 {
    config.edge_cdp_port = 9333;
}
```

#### 5. esbuild バンドル

変更後にバンドルを実行:
```bash
npx esbuild src/copilot_server.ts --bundle --platform=node --outfile=src-tauri/binaries/copilot_server.js --format=esm --external:playwright
```

---

## T27: アプリ起動時の Copilot 自動接続

### 目的
setup 完了後のアプリ起動時に copilot_server + Edge を自動で起動する。
ユーザーが何も操作しなくても Copilot 接続が確立される状態にする。

### 対象ファイル
- `src-tauri/src/lib.rs`
- `src-tauri/src/vlm/copilot_server.rs`（関数追加）

### 変更内容

#### 1. src-tauri/src/vlm/copilot_server.rs に関数を追加

ファイル末尾（`fn null_device_path()` の後）に追加:

```rust
use tauri::{AppHandle, Emitter};
use crate::state::AppState;
use crate::vlm::server::{update_vlm_state, CopilotConnectionStatus};

/// アプリ起動時に Copilot サーバーと Edge を自動接続する。
/// setup_complete かつ vlm_engine == "copilot" の場合のみ動作する。
/// 失敗してもアプリの動作には影響しない（バッチ時に再試行される）。
pub fn spawn_copilot_auto_connect(app: AppHandle, state: AppState) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = copilot_auto_connect(&app, &state).await {
            eprintln!("[copilot] auto-connect failed (will retry at batch time): {error}");
        }
    });
}

async fn copilot_auto_connect(app: &AppHandle, state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let (setup_complete, vlm_engine) = {
        let config = state.config.lock().await;
        (config.setup_complete, config.vlm_engine.clone())
    };

    if !setup_complete || vlm_engine != "copilot" {
        return Ok(());
    }

    // UI 初期化を少し待つ
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // copilot_server プロセスを起動
    let data_dir = state.app_paths.data_dir.clone();
    {
        let mut server = state.copilot_server.lock().await;
        server.start(&data_dir).await?;
    }

    let snapshot = update_vlm_state(state, Some(true), None, None).await;
    let _ = app.emit("vlm-status", &snapshot);

    // /status を呼んでログイン状態を確認
    let server_url = {
        let server = state.copilot_server.lock().await;
        server.server_url()
    };

    let status_result = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .get(format!("{server_url}/status"))
        .send()
        .await;

    if let Ok(response) = status_result {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct StatusResponse {
            connected: bool,
            login_required: bool,
        }

        if let Ok(status) = response.json::<StatusResponse>().await {
            if status.login_required {
                let _ = app.emit(
                    "copilot-login-required",
                    "Copilot にログインしてください。Edge の画面を確認してください。",
                );
            }
        }
    }

    Ok(())
}
```

#### 2. src-tauri/src/lib.rs

**import を追加:**
```rust
use vlm::copilot_server::spawn_copilot_auto_connect;
```

**setup() 内の spawn_scheduler() の直後に追加:**

```rust
spawn_scheduler(app.handle().clone(), scheduler_state);

// Copilot 自動接続（setup 完了済みの場合）
let auto_connect_state = app.state::<AppState>().inner().clone();
spawn_copilot_auto_connect(app.handle().clone(), auto_connect_state);
```

注意: `spawn_copilot_auto_connect` は `app.manage(state)` の後に呼ぶ必要がある。
現在のコードでは `app.manage(state)` → `setup_tray()` → `spawn_scheduler()` の順なので、
`spawn_scheduler()` の直後が適切。ただし `app.state::<AppState>()` で取得する。

もし `state` が `app.manage()` で consume されている場合は、事前に clone しておく:

```rust
let state = AppState::new(app.handle())?;
let scheduler_state = state.clone();
let auto_connect_state = state.clone();
// ... (既存コード)
app.manage(state);
setup_tray(app.handle())?;
spawn_scheduler(app.handle().clone(), scheduler_state);
spawn_copilot_auto_connect(app.handle().clone(), auto_connect_state);
```

#### 3. vlm/copilot_server.rs の pub 関数をモジュールから export

`spawn_copilot_auto_connect` が `lib.rs` からアクセスできるよう、
`vlm/mod.rs` で必要に応じて re-export するか、
`lib.rs` で `use vlm::copilot_server::spawn_copilot_auto_connect;` としてパスを通す。

---

## T28: バックグラウンド接続ヘルスチェックと自動再接続

### 目的
60 秒間隔で Copilot 接続を監視し、切断時に自動復旧、ログイン切れを即座に通知する。

### 対象ファイル
- `src-tauri/src/vlm/copilot_server.rs`

### 変更内容

#### 1. spawn_copilot_auto_connect() の末尾でヘルスモニターを起動

`copilot_auto_connect()` の成功後（または失敗後でも）にヘルスモニターを起動:

```rust
pub fn spawn_copilot_auto_connect(app: AppHandle, state: AppState) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = copilot_auto_connect(&app, &state).await {
            eprintln!("[copilot] auto-connect failed (will retry at batch time): {error}");
        }

        // ヘルスモニターは auto-connect の成否に関わらず開始
        let (setup_complete, vlm_engine) = {
            let config = state.config.lock().await;
            (config.setup_complete, config.vlm_engine.clone())
        };
        if setup_complete && vlm_engine == "copilot" {
            copilot_health_monitor_loop(&app, &state).await;
        }
    });
}
```

#### 2. copilot_health_monitor_loop() を追加

```rust
const HEALTH_MONITOR_INTERVAL_SECS: u64 = 60;

/// 60 秒間隔で copilot_server + Edge の接続を監視する。
/// 切断時に自動再起動、ログイン切れ時にフロントエンドに通知する。
async fn copilot_health_monitor_loop(app: &AppHandle, state: &AppState) {
    #[derive(PartialEq, Clone)]
    enum ConnectionState {
        Connected,
        LoginRequired,
        Disconnected,
    }

    let mut last_state = ConnectionState::Disconnected;

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(HEALTH_MONITOR_INTERVAL_SECS)).await;

        // バッチ実行中はスキップ（batch.rs が自前でリトライする）
        {
            let vlm_state = state.vlm_state.lock().await;
            if vlm_state.batch_running {
                continue;
            }
        }

        // エンジンが copilot でなくなった場合は終了
        {
            let config = state.config.lock().await;
            if config.vlm_engine != "copilot" {
                break;
            }
        }

        // ヘルスチェック
        let healthy = {
            let server = state.copilot_server.lock().await;
            server.health_check().await.is_ok()
        };

        if !healthy {
            // 自動再起動を試行
            let data_dir = state.app_paths.data_dir.clone();
            let restart_result = {
                let mut server = state.copilot_server.lock().await;
                server.start(&data_dir).await
            };

            match restart_result {
                Ok(()) => {
                    let snapshot = update_vlm_state(state, Some(true), None, None).await;
                    let _ = app.emit("vlm-status", &snapshot);
                    eprintln!("[copilot] health monitor: reconnected after disconnect");
                }
                Err(error) => {
                    if last_state != ConnectionState::Disconnected {
                        let snapshot = update_vlm_state(
                            state,
                            Some(false),
                            None,
                            Some(error.to_string()),
                        )
                        .await;
                        let _ = app.emit("vlm-status", &snapshot);
                        last_state = ConnectionState::Disconnected;
                    }
                    continue;
                }
            }
        }

        // /status でログイン状態を確認
        let server_url = {
            let server = state.copilot_server.lock().await;
            server.server_url()
        };

        let current_state = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .ok()
            .map(|c| c.get(format!("{server_url}/status")))
        {
            Some(request) => {
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct StatusResponse {
                    connected: bool,
                    login_required: bool,
                }

                match request.send().await {
                    Ok(response) => match response.json::<StatusResponse>().await {
                        Ok(status) if status.login_required => ConnectionState::LoginRequired,
                        Ok(status) if status.connected => ConnectionState::Connected,
                        _ => ConnectionState::Disconnected,
                    },
                    Err(_) => ConnectionState::Disconnected,
                }
            }
            None => ConnectionState::Disconnected,
        };

        // 状態が変化した場合のみイベントを emit
        if current_state != last_state {
            match &current_state {
                ConnectionState::LoginRequired => {
                    let _ = app.emit(
                        "copilot-login-required",
                        "Copilot にログインしてください。Edge の画面を確認してください。",
                    );
                }
                ConnectionState::Connected => {
                    let snapshot = update_vlm_state(state, Some(true), None, None).await;
                    let _ = app.emit("vlm-status", &snapshot);
                }
                ConnectionState::Disconnected => {
                    // すでに上の再起動失敗で emit 済み
                }
            }
            last_state = current_state;
        }
    }
}
```

### 注意事項

- `StatusResponse` 構造体が T27 と T28 で重複するので、ファイル上部に 1 つだけ定義するか、
  `CopilotConnectionStatusResponse`（server.rs に既存）を re-use すること。
  既存の `CopilotConnectionStatusResponse` を `pub` にして `use` するのが最もクリーン。

- `update_vlm_state` は `crate::vlm::server` から import する（T27 で既に追加済み）。

- ヘルスモニターは無限ループなので、アプリ終了時は `tauri::async_runtime` が自動的にキャンセルする。
  明示的なシャットダウン処理は不要。

---

## 完了条件チェックリスト

- [ ] `cargo check` がエラーなしで通過する
- [ ] `cargo test` が全テスト通過する
- [ ] `pnpm build` がエラーなしで通過する（esbuild バンドル含む）
- [ ] copilot_server.ts が `--user-data-dir` 引数を受け取れる
- [ ] `launchEdgeForCdp()` で `--user-data-dir` が Edge 起動引数に含まれる
- [ ] models.rs の edge_cdp_port デフォルトが 9333 になっている
- [ ] config.rs で 9222 → 9333 のマイグレーションが実行される
- [ ] lib.rs の setup() で spawn_copilot_auto_connect() が呼ばれている
- [ ] setup_complete && vlm_engine == "copilot" 時にアプリ起動で copilot_server が自動起動する
- [ ] 60 秒間隔のヘルスチェックループが動作する
- [ ] ヘルスチェック失敗時に copilot_server が自動再起動される
- [ ] ログイン切れ検出時に copilot-login-required イベントが emit される
- [ ] バッチ実行中はヘルスモニターがスキップされる
