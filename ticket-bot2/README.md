# ticket-bot2

`ticket-bot2` 是平行於現有 Python `ticket-bot` 的 Rust API-mode 實作。

這個目錄目前的定位很明確：

- 不碰 browser automation
- 不修改既有 Python runtime
- 專注在 tixcraft 的 API-mode watch / verify / area / ticket / order 流程

## 已完成的範圍

- `src/config.rs`
  可直接讀既有 `config.yaml`，支援 deployment profile preset、`.env` 覆蓋、`NODE_ID` profile 派生、notifications/proxy/trace env override
- `src/proxy.rs`
  round-robin proxy pool 與 session-level proxy override
- `src/cookies.rs`
  讀取 `tixcraft_cookies.json`，並避免多 session 誤用全域 cookie 檔
- `src/http_client.rs`
  `rquest` + Chrome 133 impersonation + cookie store + proxy
- `src/parser.rs`
  game / verify / area / ticket / order 解析器
- `src/captcha.rs`
  透過 Python helper 橋接現有 captcha solver
- `src/tixcraft_api.rs`
  API-mode watcher，包含：
  - game 頁目標場次解析
  - verify/check-code 流程
  - 區域挑選
  - ticket form + captcha submit
  - order / checkout polling 與 form submit
- `src/cli.rs`
  提供 `show-config`、`show-proxy`、`api-watch-dry-run`、`watch`

## 目前狀態

- `ticket-bot2` 目前適合拿來看設計、跑 parser / config / CLI / API watcher 實驗
- 目前不應視為可直接取代 Python 主線的 production runtime
- 若要給團隊參考，建議定位為「reference implementation / prototype」

## 仍然刻意沒做的部分

- Playwright / Nodriver browser automation
- Python 主線的混合模式（`api_mode=checkout`）整合
- 從瀏覽器自動同步登入狀態與 session refresh
- 多 session / 多活動的高階 orchestration 與 failover 編排
- Telegram / Discord bot orchestration
- KKTIX / Ticketmaster 平台
- 完全脫離 Python 的 captcha 推論 runtime

## 目前限制

- 目前依賴外部準備好的 `tixcraft_cookies.json`；不負責登入與 cookie 續期
- captcha 仍透過 repo root `./.venv` 的 Python helper 執行
- 目前只有 Rust CLI 路徑；尚未接回 Python CLI / TG bot / Discord bot
- fixture 與 smoke test 已補上，但仍以 parser / config / CLI 組裝驗證為主，不等於完整實戰驗證

也就是說，`ticket-bot2` 現在是「可用的 Rust API watcher」，不是「整個 ticket-bot 的 1:1 Rust rewrite」。

## 需求

- Rust toolchain (`cargo`)
- repo root 的 Python venv：`./.venv`
  因為 captcha 目前是呼叫現有 Python solver
- 可用的 `config.yaml` / `.env`
- 有效的 tixcraft cookie 檔

## 常用指令

```bash
cargo run -- --config ../config.yaml show-config
cargo run -- --config ../config.yaml show-proxy --count 5
cargo run -- --config ../config.yaml api-watch-dry-run --event IVE --fetch
cargo run -- --config ../config.yaml api-watch-dry-run --event IVE --fetch --resolve-targets --interval 3
cargo run -- --config ../config.yaml watch --event IVE --session default --interval 3
```

## 設計原則

- Rust 只接 API-mode 的高頻熱路徑
- config 語意盡量對齊 Python，避免同一份設定檔跑出不同結果
- 多 session、cookie、proxy 先顧正確性，再談 benchmark
- 先做可觀察、可測試、可替換的 watcher，再評估是否擴大 Rust 範圍

## 邊界

目前建議把 `ticket-bot2` 維持成「同 repo 的獨立子專案」。

- 共享 config、cookie、captcha helper 這類資源與契約
- 不直接依賴 Python 版內部模組與 runtime
- 邊界規則見 [BOUNDARY.md](./BOUNDARY.md)

## 對照關係

- Python `ticket_bot.config.load_config()` -> Rust `AppConfig::load_from_path()`
- Python `ProxyManager.next()` -> Rust `ProxyPool::next()`
- Python `TixcraftApiBot.watch()` -> Rust `TixcraftApiBot::watch()`

## 驗證

目前這個目錄已可執行 `cargo test` 驗證 parser、config、proxy、HTTP helper、fixture-based parser cases，以及 CLI smoke tests。
