This directory only needs the generated Copilot bridge bundle.

## Copilot エンジンのセットアップ（Node.js / Playwright）

`copilot_server.js` は `pnpm build:copilot` で自動生成されます。
別途インストールは不要ですが、実行には以下が必要です。

### 必要条件

- Node.js 18 以上（Kiroku のビルドに必須なので既存）
- Microsoft Edge がインストール済み
- M365 Copilot ライセンス

### 依存パッケージのインストール

```bash
pnpm install
```

`playwright` は devDependencies に含まれています。

### Edge のデバッグモード起動

Edge のショートカットを右クリック → プロパティ → 「リンク先」末尾に追加:

```
--remote-debugging-port=9222
```

または PowerShell で直接起動:

```powershell
Start-Process "msedge" "--remote-debugging-port=9222"
```

### M365 へのログイン

Edge を起動後、以下の URL を開いて M365 にログイン:

```
https://m365.cloud.microsoft/chat/
```

一度ログインすれば、以降は Edge を同じ方法で起動するだけで自動的に
接続されます。

### トラブルシューティング

**「Edge に接続できません」と表示される場合**
- Edge が `--remote-debugging-port=9222` 付きで起動しているか確認
- 設定画面の「Edge CDP ポート」がショートカットの番号と一致しているか確認
- `http://127.0.0.1:9222/json` をブラウザで開いてタブ一覧が表示されるか確認

**Copilot タブが見つからない場合**
- Edge で `https://m365.cloud.microsoft/chat/` を開いてから再試行
