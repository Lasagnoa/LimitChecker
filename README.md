# LimitChecker

Claude Code と Codex CLI のレートリミット使用状況を Windows タスクトレイからリアルタイムで確認できるデスクトップアプリです。

## スクリーンショット

トレイアイコンにマウスを乗せるか左クリックすると、ポップアップで使用率を確認できます。

```
Claude
5H  [■■■■□□□□□□]  42%
    3h 15m ( Reset : 05/27 18:00 (UTC+9) )
7D  [■■□□□□□□□□]  18%
    4d 2h 30m ( Reset : 05/31 12:00 (UTC+9) )

Codex
5H  [■■■■■□□□□□]  55%
    2h 45m ( Reset : 05/27 17:30 (UTC+9) )
7D  [■□□□□□□□□□]  10%
    3d 8h 0m ( Reset : 05/30 20:00 (UTC+9) )
```

## 機能

- **Claude Code** の 5時間枠・7日枠のレートリミット使用率をリアルタイム表示
- **Codex CLI** の 5時間枠・7日枠のレートリミット使用率をリアルタイム表示
- セグメント形式のビジュアルバーで使用率を直感的に把握
- リセットまでの残り時間をローカル時刻で表示
- 更新間隔を 1分 / 5分 / 15分 / 60分 から選択
- タスクトレイ常駐で常時確認可能
- 多重起動防止

## 動作環境

| 項目 | 内容 |
|------|------|
| OS | Windows 10 / Windows 11 |
| 実装言語 | Rust |
| Claude認証 | Claude Code の OAuth / Bearer token |
| Codex認証 | Codex CLI が作成する `.codex/auth.json` |

## 前提条件

以下のいずれかが必要です。

**Claude の使用率表示に必要:**

- [Claude Code](https://claude.ai/code) がインストールされ、ログイン済みであること
  - `%USERPROFILE%\.claude\.credentials.json` または `ANTHROPIC_AUTH_TOKEN` 環境変数が有効

**Codex の使用率表示に必要:**

- [Codex CLI](https://github.com/openai/codex) がインストールされ、`codex login` でログイン済みであること
  - `%USERPROFILE%\.codex\auth.json` が存在すること

> **注意:** 通常の `ANTHROPIC_API_KEY` や `OPENAI_API_KEY` では使用率を取得できません。Claude Code / Codex CLI の OAuth 認証が必要です。

## インストール

### ビルド済みバイナリを使う場合

[Releases](../../releases) から最新の `LimitChecker.exe` をダウンロードして実行してください。

`icon.ico` を `LimitChecker.exe` と同じフォルダに置くとカスタムアイコンを使用できます。

### ソースからビルドする場合

**必要なもの:**

- [Rust](https://www.rust-lang.org/tools/install) (rustup 推奨)
- Windows SDK

```powershell
git clone https://github.com/Lasagnoa/LimitChecker.git
cd LimitChecker
cargo build --release
```

ビルド成果物:

```
target\release\LimitChecker.exe
```

## 使い方

1. `LimitChecker.exe` を実行するとタスクトレイに常駐します
2. トレイアイコンに**マウスを乗せる**か**左クリック**でポップアップ表示
3. **右クリック**でメニューを開く

### 右クリックメニュー

| 項目 | 動作 |
|------|------|
| 今すぐ更新 | Claude と Codex の使用状況を即時取得 |
| 更新間隔 | 1分 / 5分 / 15分 / 60分 |
| Claude.aiを開く | `https://claude.ai` を既定ブラウザで開く |
| ChatGPTを開く | `https://chatgpt.com/` を既定ブラウザで開く |
| Claude再ログイン | `claude auth login` を実行 |
| Codex再ログイン | `codex login` を実行 |
| ログ(デバッグ用) | `limitchecker.log` 出力のON/OFF |
| 終了 | アプリを終了 |

### 設定ファイル

設定は自動的に保存されます:

```
%APPDATA%\LimitChecker\settings.json
```

## トラブルシューティング

**使用率が 0% または表示されない場合:**

- Claude Code / Codex CLI にログインしているか確認してください
- 右クリックメニューから「Claude再ログイン」「Codex再ログイン」を試してください
- 右クリックメニューから「ログ(デバッグ用)」を有効にし、`exe` と同じフォルダの `limitchecker.log` を確認してください

**WSL環境での認証:**

WSL 内の Claude Code / Codex CLI のトークンファイルも自動的に検索します。

## ライセンス

MIT License

## 技術的な詳細

詳細な仕様は [SPECIFICATION_JA.md](SPECIFICATION_JA.md) を参照してください。
