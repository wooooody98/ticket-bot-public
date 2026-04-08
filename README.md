# Ticket Bot

tixcraft 自動搶票機器人。支援 Telegram / Discord 遠端指令控制，可部署到 GCP 雲端台灣機房。

## 功能總覽

### 搶票核心
- **全自動搶票** — 場次選擇 → 驗證問答 → 區域選擇 → 勾同意 → 選票數 → 驗證碼 → 結帳
- **三種搶票模式**
  - `off` — 純瀏覽器操作（穩定，適合本機）
  - `checkout` — 瀏覽器流程 + API 結帳（較快）
  - `full` — 瀏覽器僅登入，全流程 httpx 直送（最快，適合雲端）
- **釋票監測** — 持續刷新區域頁，偵測到票自動搶，中斷自動重啟
- **NTP 精準倒數** — 同步網路時間，開賣瞬間啟動（毫秒級精度）
- **即將開賣自動刷新** — 偵測 coming soon 頁面，隨機 2-5 秒間隔刷新
- **多帳號並行** — 多 session 同時搶，第一個成功即取消其餘
- **預售驗證自動填入** — 卡號前綴 / presale code 自動填寫（jQuery AJAX 提交）

### 反偵測
- **雙引擎** — NoDriver（預設，CDP 直連）或 Playwright + stealth
- **Stealth JS 注入** — 隱藏 `navigator.webdriver`、偽造 plugins/languages、正常化 WebGL 指紋、清除 CDP 殘留變數
- **Cloudflare Turnstile 自動通過** — 模板匹配 + CDP DOM pierce fallback
- **追蹤資源封鎖** — 封鎖 16 種 GA / FB / 廣告 URL，加速載入
- **隨機延遲** — 2-5 秒隨機間隔，避免固定頻率被偵測

### 遠端控制制
- **Telegram Bot** — 指令 + 自然語言控制（「搶ITZY的票」「監測釋票」「停」）
- **Discord Bot** — 同功能 `!` 指令控制
- **驗證碼 TG 推送** — 雲端模式下驗證碼圖片推送到手機，回覆即自動填入（60 秒限時）
- **活動搜尋** — 聊天中搜尋 tixcraft 活動、自動抓取開賣時間
- **錯誤追蹤** — 最近 50 筆錯誤紀錄，可用 Claude AI 分析改善建議
- **Claude AI 輔助** — 關鍵字比對優先，fallback 到 Claude Haiku 解析自然語言

### 雲端部署
- **GCP 一鍵部署** — 打包 → 上傳 → 安裝 → 殺舊進程 → 重啟 → 自動驗證
- **Cookie 同步** — 本機登入後匯出 cookie（含 HttpOnly）到雲端 VM
- **環境變數覆蓋** — `BROWSER_HEADLESS`、`BROWSER_API_MODE`、`BROWSER_EXECUTABLE_PATH`
- **Linux 自動適配** — 自動加 `--no-sandbox`、`--disable-dev-shm-usage`、`--disable-gpu`

### 驗證碼
- **ddddocr 自動辨識** — 支援 beta 模型、自訂 ONNX 模型、信心度過濾（自動重試）
- **訓練基礎設施** — 自動收集樣本 → 互動式標記 → 轉換訓練格式
- **雲端推送** — 辨識失敗時推送圖片到 TG，用戶回覆驗證碼

### 其他
- **Proxy 輪換** — 支援多 proxy 輪替（round-robin）、單帳號指定、住宅 proxy session ID
- **Ticketmaster 監控** — Discovery API 關鍵字監控，新事件自動通知
- **Telegram / Discord 通知** — 搶票結果即時推送，含 10 分鐘付款提醒

## 環境需求

- **Python** >= 3.11
- **Chrome / Chromium**（NoDriver 引擎需要）
- **macOS / Linux**

## 安裝

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -e .

# （選用）Playwright 引擎
playwright install chromium
```

## 公開版注意

- 公開版不包含 cookies、瀏覽器 profile、私人部署設定、訓練資料與模型產物。
- 請從 `config.yaml.example`、`config.local.example.yaml`、`config.cloud.example.yaml` 或 `config.aws-tokyo.example.yaml` 複製成你自己的設定檔。
- Docker 使用者可從 `docker-compose.example.yml` 起步。
- 若要從 private repo 匯出公開版快照，請參考 `OPEN_SOURCE_RELEASE.md` 與 `scripts/release/export_public_snapshot.sh`。
- `ticket-bot2/` 是實驗性 Rust API watcher 參考實作；目前未接回 Python 主線混合模式，請先當 prototype / reference implementation 看待。

## 快速開始

### 1. 設定

```bash
cp config.yaml.example config.yaml
cp .env.example .env
```

### 2. 登入 tixcraft

```bash
ticket-bot login
```

在瀏覽器中完成 Facebook / Google 登入。

### 3. 搶票

```bash
# CLI 模式
ticket-bot run

# 或啟動 Telegram Bot 遠端控制
ticket-bot bot -p telegram
```

## 設定檔

### config.yaml

```yaml
events:
  - name: "ITZY"
    platform: tixcraft
    url: "https://tixcraft.com/activity/game/26_itzy"
    ticket_count: 2               # 張數
    date_keyword: ""              # 場次日期篩選（留空 = 第一個可用）
    area_keyword: ""              # 區域篩選（留空 = 第一個可用）
    sale_time: ""                 # 倒數模式用，ISO 8601 格式
    presale_code: ""              # 預售驗證碼（如信用卡前 6 碼）

browser:
  engine: nodriver                # nodriver（推薦）或 playwright
  headless: false                 # 雲端設 true
  user_data_dir: "./chrome_profile"
  pre_warm: true                  # 預載入頁面
  lang: "zh-TW"
  executable_path: ""             # 留空自動偵測
  api_mode: "off"                 # off / checkout / full

captcha:
  engine: ddddocr
  beta_model: true
  char_ranges: 0                  # 0 = 不限制
  confidence_threshold: 0.6
  max_attempts: 5
  preprocess: false               # tixcraft 不需前處理
  custom_model_path: ""           # 自訓練 ONNX 模型路徑
  custom_charset_path: ""         # 自訓練字元集路徑
  collect_dir: ""                 # 驗證碼收集目錄（留空不收集）

notifications:
  telegram:
    enabled: false
    chat_id: ""
  discord:
    enabled: false

proxy:
  enabled: false
  rotate: true
  servers: []

# 多帳號並行（可選）
# sessions:
#   - name: "帳號A"
#     user_data_dir: "./chrome_profile_a"
#     proxy_server: ""
```

### .env

```env
TELEGRAM_BOT_TOKEN=              # TG Bot token
TELEGRAM_CHAT_ID=                # TG 聊天室 ID
DISCORD_BOT_TOKEN=               # DC Bot token（選填）
DISCORD_WEBHOOK_URL=             # DC Webhook（選填）
ANTHROPIC_API_KEY=               # Claude API（選填，自然語言功能）
TICKETMASTER_API_KEY=            # Ticketmaster API（選填）
```

## CLI 指令

```bash
ticket-bot login                          # 開啟瀏覽器登入
ticket-bot list                           # 列出活動場次
ticket-bot run [--event X] [--date X]     # 搶票
                [--area X] [--count N]
                [--dry-run] [--parallel]
                [--api]
ticket-bot watch [--interval 3.0]         # 釋票監測
ticket-bot countdown [--parallel]         # NTP 精準倒數搶票
ticket-bot bot [-p telegram|discord|all]  # 啟動 Bot
ticket-bot monitor KEYWORDS              # Ticketmaster 事件監控
ticket-bot label [--dir PATH]            # 互動式驗證碼標記
ticket-bot prepare [--dir PATH]          # 轉換訓練資料格式
```

## Bot 指令

### Telegram（`/` 指令）

| 指令 | 別名 | 說明 |
|------|------|------|
| `/search 關鍵字` | | 搜尋 tixcraft 活動 |
| `/set URL` | | 用 URL 設定活動 |
| `/info` | `/i` | 抓取活動資訊與開賣時間 |
| `/check` | | 驗證搶票準備狀態（活動/URL/日期/區域） |
| `/saletime` | | 手動設定開賣時間 |
| `/run` | `/r` | 啟動搶票 |
| `/watch [秒數]` | `/w` | 釋票監測（預設 3 秒） |
| `/stop` | `/x` | 停止搶票/監測 |
| `/status` | `/s` | 查看狀態 |
| `/list` | `/l` | 列出已設定活動 |
| `/config [key] [val]` | `/cfg` | 查看/修改設定 |
| `/errors [N]` | | 查看最近 N 筆錯誤 |
| `/analyze [N]` | | Claude AI 分析第 N 筆錯誤 |
| `/clearerrors` | | 清除錯誤紀錄 |
| `/ping` | | 連線測試 |

**自然語言支援：**
- 搶票：「搶票」「幫我搶」「開搶」「go」
- 監測：「監測」「釋票」「有票嗎」
- 停止：「停」「別搶了」「不要了」
- 設定：「改日期 06/14」「改4張」「改區域 搖滾區」
- 查詢：「狀態」「什麼時候開賣」「在嗎」

### Discord（`!` 指令）

| 指令 | 別名 | 說明 |
|------|------|------|
| `!run [活動名稱]` | `!r` | 搶票 |
| `!watch [間隔] [活動]` | `!w` | 釋票監測 |
| `!stop` | `!x` | 停止 |
| `!status` | `!s` | 狀態 |
| `!list` | `!l` | 列出活動 |
| `!config [key] [val]` | `!cfg` | 設定 |
| `!ping` | | 連線測試 |

## API 模式

跳過瀏覽器渲染，用 httpx 直接呼叫 tixcraft 端點，大幅提升速度。

| 模式 | 瀏覽器用途 | API 處理步驟 | 速度 |
|------|----------|-------------|------|
| `off` | 全流程 | 無 | 基準 |
| `checkout` | 到結帳頁 | 結帳 POST | ~2x |
| `full` | 僅登入 | 場次+驗證+區域+結帳 | ~3-5x |

```
Full API 模式：
瀏覽器登入 → CDP 提取 Cookie（含 HttpOnly）→ httpx 全流程
  ├── GET  /activity/game/     → parse_game_list()
  ├── POST /ticket/check-code  → 驗證碼/presale 提交
  ├── GET  /ticket/area/       → parse_area_list()
  └── POST /ticket/ticket/     → 結帳送單
```

## 雲端部署（GCP）

### 建立 VM

```bash
gcloud compute instances create ticket-bot \
  --zone=asia-east1-b \
  --machine-type=e2-small \
  --image-family=ubuntu-2204-lts \
  --image-project=ubuntu-os-cloud \
  --boot-disk-size=20GB
```

### 部署腳本

| 腳本 | 用途 |
|------|------|
| `scripts/deploy/deploy.sh` | 一鍵部署（打包→上傳→安裝→殺舊→重啟→驗證） |
| `scripts/deploy/gcp_setup.sh` | VM 初始化（系統套件、Python、systemd 服務） |
| `scripts/deploy/gcp_deploy.sh <IP>` | rsync 部署 |
| `scripts/deploy/gcp_sync_profile.sh <IP>` | 同步登入狀態 |

### 雲端設定

雲端 `config.yaml` 需調整：
```yaml
browser:
  headless: true
  executable_path: "/usr/bin/chromium"
  user_data_dir: "./chrome_profile_cloud"
  api_mode: "full"
```

或用環境變數覆蓋（不動 config 檔）：
```bash
BROWSER_HEADLESS=true
BROWSER_EXECUTABLE_PATH=/usr/bin/chromium
BROWSER_API_MODE=full
```

### 雲端驗證碼流程

```
Bot 到結帳頁 → 抓驗證碼圖片 → 推送到 TG（附說明）
→ 你在手機回覆驗證碼文字 → Bot 自動填入 → 送出訂單
（限時 60 秒，超時提醒）
```

## 搶票流程圖

```
                    ┌─────────────┐
                    │   登入      │ ← 本機：瀏覽器 / 雲端：Cookie 同步
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │ 預熱頁面    │ ← 封鎖追蹤資源、載入活動頁
                    └──────┬──────┘
                           │
              ┌────────────▼────────────┐
              │ 場次選擇 /activity/game/ │ ← 偵測即將開賣 → 自動刷新
              │ 比對 date_keyword       │   全售完 → 等待刷新
              └────────────┬────────────┘
                           │
              ┌────────────▼────────────┐
              │ 驗證頁（可選）           │ ← 【答案】自動擷取
              │ /activity/verify/       │   presale_code 自動填入
              │ /ticket/verify/         │   jQuery AJAX 提交
              └────────────┬────────────┘
                           │
              ┌────────────▼────────────┐
              │ 區域選擇 /ticket/area/  │ ← 比對 area_keyword
              │                        │   全售完 → 返回場次頁
              └────────────┬────────────┘
                           │
              ┌────────────▼────────────┐
              │ 結帳 /ticket/ticket/    │ ← 勾同意、選票數
              │ 驗證碼                  │   本機：手動輸入
              │ 送出訂單               │   雲端：TG 推送→回覆
              └────────────┬────────────┘
                           │
                    ┌──────▼──────┐
                    │ 付款        │ ← 10 分鐘內完成
                    └─────────────┘
```

## 專案結構

```
ticket-bot/
├── config.yaml.example           # 設定範本
├── .env.example                  # 環境變數範本
├── pyproject.toml                # Python 套件設定
├── scripts/
│   ├── train/                    # 驗證碼資料與模型訓練
│   ├── debug/                    # 單點除錯腳本
│   └── diagnostics/              # 實戰診斷 / live probe
│   └── release/                  # 公開版快照匯出工具
├── src/ticket_bot/
│   ├── __main__.py               # python -m ticket_bot 入口
│   ├── cli.py                    # Click CLI（9 個指令）
│   ├── config.py                 # YAML + .env 設定（支援環境變數覆蓋）
│   ├── telegram_bot.py           # TG Bot（NLU + 錯誤追蹤 + 驗證碼推送）
│   ├── discord_bot.py            # DC Bot（Cog + Embed）
│   ├── browser/
│   │   ├── base.py               # 瀏覽器引擎抽象介面
│   │   ├── factory.py            # 引擎工廠（nodriver / playwright）
│   │   ├── nodriver_engine.py    # NoDriver（CDP + stealth + Cloudflare）
│   │   └── playwright_engine.py  # Playwright + stealth
│   ├── captcha/
│   │   ├── solver.py             # ddddocr OCR（重試 + 收集）
│   │   └── trainer.py            # 標記 UI + 訓練資料轉換
│   ├── platforms/
│   │   ├── tixcraft.py           # tixcraft 搶票核心（瀏覽器模式）
│   │   ├── tixcraft_api.py       # tixcraft API 高速模式（httpx）
│   │   ├── tixcraft_parser.py    # HTML 解析器（場次/驗證/區域/表單）
│   │   └── ticketmaster.py       # Ticketmaster Discovery API 監控
│   ├── notifications/
│   │   ├── telegram.py           # TG 通知推送
│   │   └── discord.py            # DC Webhook 通知
│   ├── proxy/
│   │   └── manager.py            # Proxy 輪換（round-robin + 住宅 proxy）
│   └── utils/
│       ├── retry.py              # tenacity 重試（指數退避 + 隨機延遲）
│       └── timer.py              # NTP 同步 + 毫秒級倒數
└── tests/                        # pytest 測試
```

## 速度優化

| 技巧 | 效果 |
|------|------|
| Full API 模式 | 每步 ~50ms，省去瀏覽器渲染 |
| GCP 台灣機房 | 到 tixcraft ~12ms（家用 30-200ms） |
| NoDriver CDP 直連 | 無 WebDriver 指紋 |
| Stealth JS 注入 | 7 項反偵測（webdriver/plugins/WebGL...） |
| 追蹤資源封鎖 | 省 1-3 秒頁面載入 |
| JS 直接跳轉 | `data-href` + `page.goto()`，不走 DOM click |
| Cookie 全提取 | CDP 取得 HttpOnly cookie，API 模式共用 |
| NTP 精準倒數 | 最後 2 秒 busy-wait，1ms 精度 |
| 預熱頁面 | 開賣前先載入，省首次載入延遲 |

## 主要依賴

| 套件 | 用途 |
|------|------|
| [nodriver](https://github.com/ultrafunkamsterdam/nodriver) | 反偵測瀏覽器自動化（CDP） |
| [playwright](https://playwright.dev/) + [playwright-stealth](https://pypi.org/project/playwright-stealth/) | 備選瀏覽器引擎 |
| [httpx](https://www.python-httpx.org/) | 非同步 HTTP（API 模式核心） |
| [ddddocr](https://github.com/sml2h3/ddddocr) | 驗證碼 OCR |
| [discord.py](https://discordpy.readthedocs.io/) | Discord Bot |
| [anthropic](https://docs.anthropic.com/) | Claude API（自然語言解析） |
| [ntplib](https://pypi.org/project/ntplib/) | NTP 時間同步 |
| [tenacity](https://tenacity.readthedocs.io/) | 重試機制（指數退避） |
| [click](https://click.palletsprojects.com/) | CLI 框架 |
| [pyyaml](https://pyyaml.org/) | YAML 設定 |

## 授權

MIT License。詳見 `LICENSE`。

## 使用提醒

請自行確認售票平台條款、活動規則與所在地法規，再決定是否使用本專案。
