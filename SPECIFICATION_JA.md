# Limit Checker 仕様書

## 概要

Limit Checker は、Claude と Codex のレートリミット使用状況を Windows のタスクトレイから確認するデスクトップアプリケーションです。

トレイアイコンにマウスを重ねる、または左クリックすると小さなポップアップを表示し、Claude と Codex それぞれの 5時間枠と 7日枠の使用率、リセットまでの時間を確認できます。

## 対象環境

| 項目 | 内容 |
| --- | --- |
| OS | Windows 10 / Windows 11 |
| 実装言語 | Rust |
| UI | Win32 API / タスクトレイ常駐 |
| Claude認証 | Claude Code の OAuth / Bearer token |
| Codex認証 | Codex CLI が作成する `%USERPROFILE%\.codex\auth.json` |
| 設定保存先 | `%APPDATA%\LimitChecker\settings.json` |
| ログ | exe と同じフォルダの `limitchecker.log` |

## 表示仕様

ポップアップは Claude と Codex の2セクションで構成します。

```text
Claude
5H  [使用率バー]  xx%
    リセットまでの時間 ( Reset : ローカル日時 )
7D  [使用率バー]  xx%
    リセットまでの時間 ( Reset : ローカル日時 )

Codex
5H  [使用率バー]  xx%
    リセットまでの時間 ( Reset : ローカル日時 )
7D  [使用率バー]  xx%
    リセットまでの時間 ( Reset : ローカル日時 )
```

### 色

| 対象 | 色 | 備考 |
| --- | --- | --- |
| 背景 | `#1C1C1C` | ダーク背景 |
| Claudeバー | `#D97757` | Rustコード上はBGRで `0x005777D9` |
| Codexバー | `#6699CC` | Rustコード上はBGRで `0x00CC9966` |
| 未使用バー | `#444444` | セグメント背景 |
| テキスト | `#FFFFFF` | 通常テキスト |

## レートリミット取得仕様

### Claude

Claude は Claude Code の OAuth/Bearer token を使い、まず Claude Code OAuth 使用量 API から 5時間枠と 7日枠の使用率を読みます。
OAuth 使用量 API が失敗した場合のみ、従来どおり Messages API のレスポンスヘッダーからの取得にフォールバックします。

認証情報は Claude Code の現行仕様に近い順序で確認します。

1. `CLAUDE_CODE_USE_BEDROCK` / `CLAUDE_CODE_USE_VERTEX` / `CLAUDE_CODE_USE_FOUNDRY`
   - クラウドプロバイダ認証はこのアプリの5H/7D OAuth使用量取得では非対応
   - ログに理由を出し、前回表示を保持
2. `ANTHROPIC_AUTH_TOKEN`
   - Bearer token として使用
3. `ANTHROPIC_API_KEY`
   - 通常APIキーでは Claude Code の5H/7D OAuth使用量ヘッダーを取得できないため非対応
   - ログに理由を出し、前回表示を保持
4. `CLAUDE_CODE_OAUTH_TOKEN`
   - Bearer token として使用
5. `$CLAUDE_CONFIG_DIR\.credentials.json`
6. `%USERPROFILE%\.claude\.credentials.json`
7. WSL環境の `.claude/.credentials.json`
8. 有効なトークンがない場合は `claude -p "say hi" --max-turns 1` でリフレッシュを試し、同じ候補ファイルを再読込

`ANTHROPIC_AUTH_TOKEN` と `CLAUDE_CODE_OAUTH_TOKEN` は、値が `Bearer xxx` 形式でも `xxx` 形式でも扱えます。

Claude OAuth 使用量取得リクエスト:

| 項目 | 内容 |
| --- | --- |
| URL | `https://api.anthropic.com/api/oauth/usage` |
| 認証 | `Authorization: Bearer <token>` |
| Beta | `anthropic-beta: oauth-2025-04-20` |

使用する主なJSON項目:

| JSONパス | 意味 |
| --- | --- |
| `/five_hour/utilization` | 5時間枠の使用率。値はパーセント |
| `/five_hour/resets_at` | 5時間枠のリセット時刻 |
| `/seven_day/utilization` | 7日枠の使用率。値はパーセント |
| `/seven_day/resets_at` | 7日枠のリセット時刻 |

`seven_day` がない場合は、`seven_day_opus` / `seven_day_sonnet` / `seven_day_haiku` / `seven_day_oauth_apps` のうち使用率が最大のものを7日枠として表示します。
reset時刻は Unix epoch 秒と RFC3339 の両方を読めます。

Messages API フォールバック取得リクエスト:

| 項目 | 内容 |
| --- | --- |
| URL | `https://api.anthropic.com/v1/messages` |
| 認証 | `Authorization: Bearer <token>` |
| バージョン | `anthropic-version: 2023-06-01` |
| Beta | `anthropic-beta: oauth-2025-04-20` |
| モデル | `claude-haiku-4-5-20251001` |
| リクエスト内容 | `max_tokens: 1` の最小メッセージ |

使用する主なヘッダー:

| ヘッダー | 意味 |
| --- | --- |
| `anthropic-ratelimit-unified-5h-utilization` | 5時間枠の使用率。値はパーセントではなく `0.0 - 1.0+` の比率 |
| `anthropic-ratelimit-unified-5h-status` | 5時間枠の状態 |
| `anthropic-ratelimit-unified-5h-reset` | 5時間枠のリセット時刻 |
| `anthropic-ratelimit-unified-7d-utilization` | 7日枠の使用率。値はパーセントではなく `0.0 - 1.0+` の比率 |
| `anthropic-ratelimit-unified-7d-status` | 7日枠の状態 |
| `anthropic-ratelimit-unified-7d-reset` | 7日枠のリセット時刻 |
| `anthropic-ratelimit-unified-status` | 全体状態 |
| `anthropic-ratelimit-unified-reset` | 個別resetがない場合のフォールバック |

利用率の扱い:

- `0.42` は `42%` として表示
- `1.02` は `102%` として表示
- `rate_limited` / `exceeded` / `rejected` は利用率ヘッダーがなくても `100%` 扱い
- HTTP 429 で unified ヘッダーがない場合も `100%` 扱い
- reset時刻は Unix epoch 秒と RFC3339 の両方を読める

### Codex

Codex は Codex CLI の ChatGPTログイン用 OAuth token を使います。

1. `%USERPROFILE%\.codex\auth.json` を読む
2. WSL環境の `.codex/auth.json` も候補にする
3. `tokens.access_token` を使用する
4. `https://chatgpt.com/backend-api/wham/usage` を直接呼び出す
5. JSONレスポンスの `rate_limit.primary_window` と `rate_limit.secondary_window` を使用する

使用する主なJSON項目:

| JSONパス | 意味 |
| --- | --- |
| `/rate_limit/primary_window/used_percent` | 5時間枠の使用率 |
| `/rate_limit/primary_window/reset_at` | 5時間枠のリセット時刻 |
| `/rate_limit/primary_window/reset_after_seconds` | 5時間枠のリセットまでの秒数 |
| `/rate_limit/secondary_window/used_percent` | 7日枠の使用率 |
| `/rate_limit/secondary_window/reset_at` | 7日枠のリセット時刻 |
| `/rate_limit/secondary_window/reset_after_seconds` | 7日枠のリセットまでの秒数 |

`OPENAI_API_KEY` や `codex login --with-api-key` で使うAPIキー認証では、現在のChatGPT/Codex使用量APIは取得対象外です。

## エラー時の表示

- Claude または Codex の取得に失敗しても、失敗した側は前回表示を保持します。
- 片方の取得失敗で、もう片方の表示は止めません。
- API取得エラーはポップアップには表示せず、ログON時のみ `limitchecker.log` に記録します。
- 初回起動時など前回値がない場合はデフォルトの `0%` 表示になります。

## メニュー仕様

右クリックメニュー:

| 項目 | 動作 |
| --- | --- |
| 今すぐ更新 | Claude と Codex の使用状況を即時取得 |
| 更新間隔 | 1分 / 5分 / 15分 / 60分 |
| Claude.aiを開く | `https://claude.ai` を既定ブラウザで開く |
| ChatGPTを開く | `https://chatgpt.com/` を既定ブラウザで開く |
| Claude再ログイン | `claude auth login` を実行 |
| Codex再ログイン | `codex login` を実行 |
| ログ(デバッグ用) | `limitchecker.log` 出力のON/OFF |
| status.json を出力 | `status.json` 書き出しのON/OFF |
| 終了 | アプリを終了 |

## 設定仕様

設定は `%APPDATA%\LimitChecker\settings.json` に保存します。

| 項目 | 内容 |
| --- | --- |
| `poll_interval_ms` | 更新間隔 |
| `log_enabled` | デバッグログON/OFF |
| `status_json_enabled` | status.json 書き出しON/OFF (デフォルト: ON) |

## 状態ファイル仕様 (status.json)

外部ツール (AIアシスタント等) から現在のレートリミット状況を読み取れるよう、ポーリングが完了するたびに `%APPDATA%\LimitChecker\status.json` を上書きします。

| 項目 | 型 | 意味 |
| --- | --- | --- |
| `updated_at` | string (RFC3339, ローカルTZ付き) | このファイルが書かれた時刻 |
| `schema_version` | number | スキーマバージョン。フォーマット変更時にインクリメント |
| `app_version` | string | LimitChecker のバージョン |
| `claude_configured` | bool | Claude のトークンが見つかったか |
| `claude.session_pct` | number | 5時間枠の使用率 (0〜100) |
| `claude.session_resets_at` | string \| null | 5時間枠のリセット時刻 (RFC3339, UTC) |
| `claude.weekly_pct` | number | 7日枠の使用率 |
| `claude.weekly_resets_at` | string \| null | 7日枠のリセット時刻 |
| `codex_configured` | bool | Codex のトークンが見つかったか |
| `codex.session_pct` | number | 5時間枠の使用率 |
| `codex.session_resets_at` | string \| null | 5時間枠のリセット時刻 |
| `codex.weekly_pct` | number | 7日枠の使用率 |
| `codex.weekly_resets_at` | string \| null | 7日枠のリセット時刻 |

書き出しは `poll_once` の最後で行います。書き出し失敗はログにのみ出ます。

右クリックメニューの「status.json を出力」でON/OFF を切り替えられます。OFF にした時は既存の `status.json` を削除します。ON に戻した時は即時にポーリングを走らせて最新値で書き直します。`--once` も `status_json_enabled` に従いますが、標準出力への JSON は設定に関わらず常に出力します。

## サブコマンド仕様

| サブコマンド | 動作 |
| --- | --- |
| (引数なし) | 通常のタスクトレイ常駐モード |
| `--once` | 1回だけ使用量を取得して `status.json` を更新し、同じ JSON を標準出力にも書き出して終了する。常駐インスタンスと並行して呼んでも問題なし |

`--once` は `#![windows_subsystem = "windows"]` ビルドでも、PowerShell/cmd から呼ばれた場合は `AttachConsole(ATTACH_PARENT_PROCESS)` で親コンソールに接続して結果を表示します。GUI からダブルクリックで起動された場合は標準出力先がないため、結果は `status.json` への書き出しのみになります。

## モジュール構成

| ファイル | 役割 |
| --- | --- |
| `src/main.rs` | Win32 UI、タスクトレイ、ポップアップ描画、メニュー |
| `src/poller.rs` | Claude/Codex の使用状況取得、ログ出力、時刻フォーマット |
| `src/credentials.rs` | Claude/Codex の認証トークン読み取り |
| `src/settings.rs` | 設定の保存と読み込み |
| `build.rs` | Windows exe アイコン埋め込み |

## ビルド

```powershell
cargo build --release
```

成果物:

```text
target\release\LimitChecker.exe
```

## テスト

```powershell
cargo test
```

現在の主なテスト対象:

- Claude unified utilization の比率変換
- `1.02` を `102%` として扱うこと
- rate limit 状態の100%フォールバック
- 429時の100%フォールバック
- reset時刻の Unix epoch 秒 / RFC3339 パース

## 注意事項

- Claude の `anthropic-ratelimit-unified-*` ヘッダーは Claude Code OAuth/Bearer token 前提です。通常の `ANTHROPIC_API_KEY` ではこのアプリの5H/7D表示には使いません。
- Codexの使用量取得は ChatGPT/Codex ログイン用の OAuth token を使うため、通常の `OPENAI_API_KEY` ではなく `.codex/auth.json` の `tokens.access_token` を使用します。
- Claude/Codex とも、公式に公開されている安定APIだけでなく CLI の認証情報や内部向けに近い使用量API/ヘッダーに依存しています。仕様変更時は `limitchecker.log` を有効にして確認してください。
