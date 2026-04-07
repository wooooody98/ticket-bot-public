# ticket-bot2 Boundary

`ticket-bot2` 目前最適合的定位是：

- 同 repo 的獨立子專案
- 和 Python `ticket-bot` 共用少量 runtime 資源
- 不直接耦合 Python 內部模組或執行流程

這份文件的目的是把邊界畫清楚，避免後面開發時兩邊越纏越深。

## 現階段原則

- `ticket-bot2` 留在同一個 repo，不急著拆成獨立 repo
- `ticket-bot2` 只處理 Rust API-mode watcher
- Python 版繼續作為主線可用版本
- 共用的是資源和契約，不是內部實作

## 可以共享的東西

- `config.yaml` 的 schema 與欄位語意
- `.env` 內的部署與通知設定
- `tixcraft_cookies.json` 這類 cookie 檔格式
- repo root 的 `.venv`，目前只用在 captcha helper bridge
- 測試用 fixture、sample HTML、debug 資料

## 不應該直接依賴的東西

- `ticket-bot2` 不應 import 或呼叫 `src/ticket_bot/**` 的 Python 模組
- `ticket-bot2` 不應直接依賴 Python bot 的 runtime side effect
- `ticket-bot2` 不應假設 Telegram / Discord / browser automation 一定存在
- `ticket-bot2` 不應修改 Python 版資料結構來遷就 Rust
- Python 版也不應反過來直接依賴 Rust crate 內部實作
- `ticket-bot2` 目前不應被視為 Python `api_mode=checkout` 混合模式的直接替代品

## 建議的共享方式

- config：共享 YAML schema，不共享 parser 實作
- captcha：共享一個穩定的 CLI helper 介面，不共享 Python 內部 class
- cookies：共享 JSON 格式，不共享 session 管理邏輯
- proxy：共享設定格式，不共享狀態物件
- parser 行為：共享測試案例與 sample page，不共享語言層級的實作

## 目前推薦的結構

```text
ticket-bot/
  src/ticket_bot/          # 既有 Python 主線
  tests/                   # Python tests
  ticket-bot2/
    src/                   # Rust watcher
    scripts/               # Rust 專用 helper
    README.md
    BOUNDARY.md
    Cargo.toml
```

## 實作規則

- `ticket-bot2` 的所有入口都從 CLI 參數讀 config path
- 共享資源都視為外部輸入，不要寫死 repo 內部 import 路徑
- 若需要和 Python 溝通，優先走檔案格式或 CLI bridge
- 新功能先問自己：這是共享契約，還是偷偷共用內部邏輯
- 只要答案偏向後者，就先停下來改成明確介面

## 近期最合理的方向

- 補齊 Rust API watcher 的 proxy / session failover
- 補更多 parser 與 flow 測試
- 把 captcha helper 介面固定下來
- 減少對 repo root `.venv` 的隱性依賴
- 若要真正導入主線，再評估如何接回 Python 的混合模式與登入/session 管理

## 什麼時候才值得拆 repo

符合下面多數條件時，再考慮把 `ticket-bot2` 拆出去：

- `ticket-bot2` 可以不依賴 repo root `.venv`
- captcha 不再需要 Python helper，或 bridge 已穩定到像外部依賴
- Rust 版有自己的 release 節奏
- Rust 版有自己的 CI / benchmark / deployment
- config schema 已經穩定，不會每週跟 Python 一起變
- 你想把 Rust 版單獨開源或單獨發版

## 拆 repo 前要先完成的事情

- 做一份 `config.example.yaml` 給 `ticket-bot2`
- 明確定義 captcha helper input/output
- 明確定義 cookie 檔格式與搜尋規則
- 把共用測試 fixture 搬到雙方都能引用的位置
- 讓 `cargo run` 不依賴 repo root 相對路徑猜測

一句話結論：

現在適合的是「同 repo、共享資源、嚴格邊界」；
不是「立刻拆 repo」，也不是「直接和 Python 版混成一坨」。
