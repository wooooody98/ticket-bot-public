use crate::captcha::CaptchaSolverBridge;
use crate::config::{AppConfig, EventConfig, SessionConfig};
use crate::cookies::{self, CookieEntry};
use crate::http_client::ApiHttpClient;
use crate::parser::{
    detect_coming_soon, detect_login_required, matches_keyword, parse_area_list, parse_game_list,
    parse_order_form, parse_ticket_form, parse_verify_page,
};
use crate::proxy::ProxyPool;
use anyhow::{Context, Result};
use rand::random;
use regex::Regex;
use std::path::Path;
use std::time::Instant;
use tracing::{info, warn};

pub const BASE_URL: &str = "https://tixcraft.com";

const BLOCKED_CODES: &[u16] = &[401, 403];
const FAIL_PATTERNS: &[&str] = &[
    "/ticket/ticket/",
    "/ticket/area/",
    "/activity/game/",
    "/activity/detail/",
];

#[derive(Debug, Clone)]
pub struct BotPlan {
    pub event_name: String,
    pub event_url: String,
    pub session_name: String,
    pub session_profile: String,
    pub cookie_file: String,
    pub browser_api_mode: String,
    pub proxy: Option<String>,
    pub user_agent: String,
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub status: u16,
    pub final_url: String,
    pub content_type: String,
    pub body_len: usize,
}

#[derive(Debug, Clone)]
pub struct WatchTargetPreview {
    pub keyword: String,
    pub text: String,
    pub href: String,
}

#[derive(Debug, Clone)]
pub struct WatchPreview {
    pub target_count: usize,
    pub request_gap_secs: f64,
    pub target_refresh_secs: f64,
    pub targets: Vec<WatchTargetPreview>,
}

#[derive(Debug, Default)]
pub struct WatchStats {
    pub ok: u64,
    pub blocked: u64,
    pub total: u64,
    pub latency_sum_ms: f64,
}

impl WatchStats {
    fn record(&mut self, status: u16, latency_ms: f64) {
        self.total += 1;
        if BLOCKED_CODES.contains(&status) {
            self.blocked += 1;
        } else {
            self.ok += 1;
        }
        self.latency_sum_ms += latency_ms;
    }

    pub fn success_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.ok as f64 / self.total as f64
        }
    }

    pub fn avg_latency_ms(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.latency_sum_ms / self.total as f64
        }
    }
}

#[derive(Debug, Clone)]
struct WatchTarget {
    keyword: String,
    text: String,
    href: String,
    visits: u64,
}

pub struct TixcraftApiBot {
    config: AppConfig,
    event: EventConfig,
    session: SessionConfig,
    proxy_pool: ProxyPool,
    cookie_entries: Vec<CookieEntry>,
    proxy: Option<String>,
    http: ApiHttpClient,
    captcha: CaptchaSolverBridge,
    stats: WatchStats,
    last_error: String,
    last_success_info: String,
}

impl TixcraftApiBot {
    pub fn new(config: &AppConfig, event: EventConfig, session: SessionConfig) -> Result<Self> {
        let proxy_pool = ProxyPool::new(config.proxy.clone());
        let proxy = proxy_pool.resolve_for_session(&session);
        let resolved_cookie_file = Self::resolve_cookie_file(config, &session);
        let cookie_entries = match resolved_cookie_file.as_deref() {
            Some(cookie_file) if Path::new(cookie_file).exists() => {
                match cookies::load_cookies(cookie_file) {
                    Ok(entries) => entries,
                    Err(error) => {
                        warn!("cookie 載入失敗 ({}): {}", cookie_file, error);
                        Vec::new()
                    }
                }
            }
            Some(cookie_file) => {
                warn!("找不到 cookie 檔：{}", cookie_file);
                Vec::new()
            }
            None => Vec::new(),
        };

        let http = ApiHttpClient::new(proxy.as_deref(), &cookie_entries)?;
        let captcha = CaptchaSolverBridge::new(config.captcha.clone());
        info!(
            cookies = cookie_entries.len(),
            cookie_file = resolved_cookie_file.as_deref().unwrap_or("-"),
            proxy = proxy.as_deref().unwrap_or("-"),
            "API session 初始化完成"
        );

        Ok(Self {
            config: config.clone(),
            event,
            session,
            proxy_pool,
            cookie_entries,
            proxy,
            http,
            captcha,
            stats: WatchStats::default(),
            last_error: String::new(),
            last_success_info: String::new(),
        })
    }

    pub fn plan(&self) -> BotPlan {
        BotPlan {
            event_name: self.event.name.clone(),
            event_url: self.event.url.clone(),
            session_name: self.session.name.clone(),
            session_profile: self.session.user_data_dir.clone(),
            cookie_file: Self::resolve_cookie_file(&self.config, &self.session).unwrap_or_default(),
            browser_api_mode: self.config.browser.api_mode.clone(),
            proxy: self.proxy.clone(),
            user_agent: self.http.user_agent().to_string(),
        }
    }

    pub fn stats(&self) -> &WatchStats {
        &self.stats
    }

    pub fn last_error(&self) -> &str {
        &self.last_error
    }

    pub fn last_success_info(&self) -> &str {
        &self.last_success_info
    }

    pub async fn probe_event(&self) -> Result<ProbeResult> {
        let response = self
            .http
            .get_text_following_redirects(&self.event.url, None)
            .await?;
        Ok(ProbeResult {
            status: response.status,
            final_url: response.final_url,
            content_type: response.content_type.unwrap_or_default(),
            body_len: response.body.len(),
        })
    }

    pub async fn preview_watch_targets(&self, interval_secs: f64) -> Result<WatchPreview> {
        let targets = self.resolve_watch_targets().await?;
        let target_count = targets.len();
        let request_gap_secs = Self::watch_request_gap_seconds(interval_secs, target_count);
        let target_refresh_secs = Self::watch_target_refresh_seconds(interval_secs, target_count);

        Ok(WatchPreview {
            target_count,
            request_gap_secs,
            target_refresh_secs,
            targets: targets
                .into_iter()
                .map(|target| WatchTargetPreview {
                    keyword: target.keyword,
                    text: target.text,
                    href: target.href,
                })
                .collect(),
        })
    }

    pub async fn watch(&mut self, interval_secs: f64) -> Result<()> {
        let interval = tokio::time::Duration::from_secs_f64(Self::watch_request_gap_seconds(
            interval_secs,
            1,
        ));
        let mut targets = loop {
            match self.resolve_watch_targets().await {
                Ok(targets) => break targets,
                Err(error) if Self::is_blocked_error_message(&error.to_string()) => {
                    self.last_error = error.to_string();
                    warn!(
                        "初始化監測目標失敗: {}，重建 HTTP client 後重試",
                        self.last_error
                    );
                    self.rebuild_http_client("resolve_watch_targets")?;
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
                Err(error) => return Err(error),
            }
        };
        info!(
            targets = targets.len(),
            request_gap = Self::watch_request_gap_seconds(interval_secs, targets.len()),
            target_refresh = Self::watch_target_refresh_seconds(interval_secs, targets.len()),
            "開始監測釋票: {}",
            self.event.name,
        );
        for target in &targets {
            info!(
                "  場次 [{}]: {} → {}",
                target.keyword, target.text, target.href
            );
        }

        let mut round: u64 = 0;
        let mut watch_index = 0;
        let stats_interval = 50_u64;

        loop {
            round += 1;
            let target = &mut targets[watch_index];
            target.visits += 1;

            let t0 = Instant::now();
            let response = self.http.get_text(&target.href, None).await;
            let latency_ms = t0.elapsed().as_secs_f64() * 1000.0;

            match response {
                Ok(resp) => {
                    self.stats.record(resp.status, latency_ms);

                    if BLOCKED_CODES.contains(&resp.status) {
                        if target.visits <= 3 || target.visits % 10 == 0 {
                            warn!(
                                "[{}] 第 {} 輪收到 {}，{:.0}ms",
                                &target.text[..target.text.len().min(24)],
                                round,
                                resp.status,
                                latency_ms,
                            );
                        }
                        self.last_error = format!("watch 收到 {}，重建 HTTP client", resp.status);
                        self.rebuild_http_client(&format!("watch_status_{}", resp.status))?;
                    } else if let Some(location) = resp.location.as_deref() {
                        let next_url = Self::normalize_url(location);
                        if detect_login_required("", &next_url) {
                            self.last_error = "watch 被導向登入頁，cookie 可能已過期".to_string();
                            anyhow::bail!(self.last_error.clone());
                        }
                        if next_url.contains("/ticket/area/") {
                            target.href = next_url;
                        } else {
                            self.last_error = format!("watch 被導向非 area 頁面: {}", next_url);
                            warn!("{}", self.last_error);
                        }
                    } else if detect_login_required(&resp.body, &resp.final_url) {
                        self.last_error = "watch 收到登入頁，cookie 可能已過期".to_string();
                        anyhow::bail!(self.last_error.clone());
                    } else {
                        let area_info = parse_area_list(&resp.body);
                        let selectable = Self::filter_selectable_areas(area_info.available);

                        if selectable.is_empty() {
                            if target.visits % 10 == 1 {
                                let sold_out = area_info.sold_out.len();
                                info!(
                                    "[{}] [第 {} 輪/{} 次] 尚無可用票券, sold out {}/{}, {:.0}ms",
                                    &target.text[..target.text.len().min(24)],
                                    round,
                                    target.visits,
                                    sold_out,
                                    area_info.total,
                                    latency_ms,
                                );
                            }
                        } else {
                            info!("偵測到 {} 個可用區域", selectable.len());
                            let selected_area = self.select_area(&selectable);
                            info!("選擇區域: {} → {}", selected_area.text, selected_area.url);

                            if self
                                .fill_ticket_form_api(&selected_area.url, &target.text)
                                .await?
                            {
                                return Ok(());
                            }

                            warn!("本輪送單失敗，繼續監測: {}", self.last_error);
                        }
                    }
                }
                Err(error) => {
                    warn!("[{}] 第 {} 輪請求失敗: {}", target.text, round, error);
                    self.last_error = format!("watch 請求失敗: {}", error);
                    self.rebuild_http_client("watch_request_error")?;
                }
            }

            if self.stats.total > 0 && self.stats.total.is_multiple_of(stats_interval) {
                info!(
                    "Watch 統計: {:.0}% ok ({}/{}), avg {:.0}ms",
                    self.stats.success_rate() * 100.0,
                    self.stats.ok,
                    self.stats.total,
                    self.stats.avg_latency_ms(),
                );
            }

            watch_index = (watch_index + 1) % targets.len();
            tokio::time::sleep(interval).await;
        }
    }

    async fn resolve_watch_targets(&self) -> Result<Vec<WatchTarget>> {
        let game_url = self.game_url();
        let response = self
            .http
            .get_text_following_redirects(&game_url, None)
            .await?;

        if BLOCKED_CODES.contains(&response.status) {
            anyhow::bail!("game 頁收到 {} (可能被 Cloudflare 擋)", response.status);
        }
        if detect_login_required(&response.body, &response.final_url) {
            anyhow::bail!("game 頁顯示登入頁，cookie 可能已過期");
        }

        let rows = parse_game_list(&response.body);
        if rows.is_empty() {
            if detect_coming_soon(&response.body) {
                anyhow::bail!("game 頁顯示即將開賣，尚未產生可監測場次");
            }
            anyhow::bail!("game 頁找不到場次列表");
        }

        let selected_rows = Self::select_game_rows(&rows, &self.event.date_keyword);

        let mut targets = Vec::with_capacity(selected_rows.len());
        for (text, href) in selected_rows {
            targets.push(self.resolve_watch_target(text, href).await?);
        }

        Ok(targets)
    }

    async fn resolve_watch_target(&self, text: String, href: String) -> Result<WatchTarget> {
        let mut resolved_href = Self::normalize_url(&href);
        if Self::is_verify_url(&resolved_href) {
            resolved_href = self
                .handle_verify_api(&resolved_href)
                .await?
                .with_context(|| format!("verify 頁無法換到 area URL: {}", text))?;
        }

        Ok(WatchTarget {
            keyword: text.clone(),
            text,
            href: resolved_href,
            visits: 0,
        })
    }

    async fn handle_verify_api(&self, verify_url: &str) -> Result<Option<String>> {
        let verify_url = Self::normalize_url(verify_url);
        let response = self
            .http
            .get_text_following_redirects(&verify_url, None)
            .await?;
        if BLOCKED_CODES.contains(&response.status) {
            anyhow::bail!("verify 頁收到 {} (可能被 Cloudflare 擋)", response.status);
        }

        let info = parse_verify_page(&response.body);
        let answer = if let Some(answer) = info.answer {
            answer
        } else if !self.event.presale_code.trim().is_empty() {
            self.event.presale_code.clone()
        } else {
            warn!("verify 頁無法取得答案，且未設定 presale_code");
            return Ok(None);
        };

        let csrf = info.csrf.unwrap_or_default();
        let check_url = info
            .form_action
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(Self::normalize_url)
            .unwrap_or_else(|| Self::verify_check_url(&verify_url));
        let post = self
            .http
            .post_form(
                &check_url,
                Some(&verify_url),
                &[("_csrf", csrf.as_str()), ("checkCode", answer.as_str())],
            )
            .await?;

        if let Some(location) = post.location.as_deref() {
            let location = Self::normalize_url(location);
            if !Self::is_verify_url(&location) {
                return Ok(Some(location));
            }
        }

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&post.body) {
            if let Some(url) = json.get("url").and_then(|value| value.as_str()) {
                let url = Self::normalize_url(url);
                if !Self::is_verify_url(&url) {
                    return Ok(Some(url));
                }
            }
            if let Some(message) = json.get("message").and_then(|value| value.as_str()) {
                warn!("verify 失敗: {}", message);
            }
        }

        if !post.final_url.is_empty() && !Self::is_verify_url(&post.final_url) {
            return Ok(Some(post.final_url));
        }

        warn!("verify 回應異常 (status={})", post.status);
        Ok(None)
    }

    async fn fill_ticket_form_api(&mut self, ticket_url: &str, target_text: &str) -> Result<bool> {
        let ticket_url = Self::normalize_url(ticket_url);
        let response = self
            .http
            .get_text_following_redirects(&ticket_url, None)
            .await?;
        if detect_login_required(&response.body, &response.final_url) {
            self.last_error = "票頁需要重新登入".to_string();
            return Ok(false);
        }

        let ticket_form = parse_ticket_form(&response.body);
        if !ticket_form.fields.contains_key("_csrf") {
            self.last_error =
                self.classify_ticket_page_failure(&response.body, &response.final_url);
            return Ok(false);
        }

        let mut fields = ticket_form.fields;
        let desired_count = self.event.ticket_count.max(1);
        if let Some(select_name) = ticket_form.select_name.clone() {
            let selected_count =
                Self::select_ticket_count(&ticket_form.select_options, desired_count)
                    .with_context(|| format!("票頁沒有可用票數選項: {}", ticket_url))?;
            if selected_count != desired_count {
                warn!("目標票數 {} 不可用，改用 {}", desired_count, selected_count);
            }
            fields.insert(select_name, selected_count.to_string());
        }

        let captcha_text = self.solve_captcha_api(&ticket_url).await?;
        if captcha_text.len() != 4 {
            self.last_error = "驗證碼辨識失敗".to_string();
            return Ok(false);
        }
        fields.insert("TicketForm[verifyCode]".to_string(), captcha_text.clone());

        let payload = Self::build_form_payload(&fields);
        let post = self
            .http
            .post_form(&ticket_url, Some(&ticket_url), &payload)
            .await?;

        let next_url = post
            .location
            .as_deref()
            .map(Self::normalize_url)
            .or_else(|| {
                if post.final_url != ticket_url {
                    Some(post.final_url.clone())
                } else {
                    None
                }
            });

        if let Some(next_url) = next_url {
            if Self::is_fail_url(&next_url) {
                self.last_error = format!("送單後被踢回: {}", next_url);
                return Ok(false);
            }
            if Self::is_success_order_url(&next_url) {
                self.last_success_info = format!(
                    "場次: {}\n區域 URL: {}\n張數: {}",
                    target_text, ticket_url, desired_count
                );
                return Ok(true);
            }
            if Self::is_checkout_url(&next_url) {
                let ok = self.handle_order_api(&next_url).await?;
                if ok {
                    self.last_success_info = format!(
                        "場次: {}\n票頁: {}\n張數: {}",
                        target_text, ticket_url, desired_count
                    );
                }
                return Ok(ok);
            }

            self.last_error = format!("送單後跳到未知頁面: {}", next_url);
            return Ok(false);
        }

        self.last_error = Self::extract_error_message(&post.body)
            .unwrap_or_else(|| format!("送單失敗：HTTP {}（非 redirect）", post.status));
        Ok(false)
    }

    async fn solve_captcha_api(&self, referer_url: &str) -> Result<String> {
        let image = self.fetch_captcha_image(referer_url).await?;
        if image.is_empty() {
            return Ok(String::new());
        }
        self.captcha.solve(&image).await
    }

    async fn fetch_captcha_image(&self, referer_url: &str) -> Result<Vec<u8>> {
        let refresh = format!("{}", random::<f64>());
        let response = self
            .http
            .get_binary(
                &format!("{BASE_URL}/ticket/captcha"),
                &[("refresh", refresh)],
                Some(referer_url),
            )
            .await?;

        if response.status != 200 {
            anyhow::bail!("captcha endpoint returned {}", response.status);
        }

        if response
            .content_type
            .as_deref()
            .unwrap_or_default()
            .contains("json")
        {
            let json: serde_json::Value = serde_json::from_slice(&response.body)?;
            let Some(url) = json.get("url").and_then(|value| value.as_str()) else {
                return Ok(Vec::new());
            };
            let image = self
                .http
                .get_binary(&Self::normalize_url(url), &[], Some(referer_url))
                .await?;
            return Ok(image.body);
        }

        Ok(response.body)
    }

    async fn handle_order_api(&mut self, order_url: &str) -> Result<bool> {
        let mut current_url = Self::normalize_url(order_url);
        let mut referer = current_url.clone();

        for _step in 0..5 {
            let mut form = None;
            let mut redirected = false;

            for _poll in 0..100 {
                let response = self.http.get_text(&current_url, Some(&referer)).await?;

                if let Some(location) = response.location.as_deref() {
                    let next_url = Self::normalize_url(location);
                    if Self::is_fail_url(&next_url) {
                        self.last_error = format!("Sit tight 被踢回: {}", next_url);
                        return Ok(false);
                    }
                    if Self::is_success_order_url(&next_url) {
                        return Ok(true);
                    }
                    if Self::is_checkout_url(&next_url) {
                        referer = current_url.clone();
                        current_url = next_url;
                        redirected = true;
                        break;
                    }
                }

                if detect_login_required(&response.body, &response.final_url) {
                    self.last_error = "結帳頁需要重新登入".to_string();
                    return Ok(false);
                }

                form = parse_order_form(&response.body);
                if form.is_some() {
                    break;
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            }

            if redirected {
                continue;
            }

            let Some(form) = form else {
                self.last_error = "等待結帳表單逾時".to_string();
                return Ok(false);
            };

            let payload = Self::build_form_payload(&form.fields);
            let post = self
                .http
                .post_form(&current_url, Some(&current_url), &payload)
                .await?;

            let next_url = post
                .location
                .as_deref()
                .map(Self::normalize_url)
                .unwrap_or_default();
            if next_url.is_empty() {
                self.last_error = Self::extract_error_message(&post.body)
                    .unwrap_or_else(|| format!("結帳 POST 失敗：HTTP {}", post.status));
                return Ok(false);
            }
            if Self::is_fail_url(&next_url) {
                self.last_error = format!("Checkout 後被踢回: {}", next_url);
                return Ok(false);
            }
            if Self::is_success_order_url(&next_url) {
                return Ok(true);
            }
            if Self::is_checkout_url(&next_url) {
                current_url = next_url;
                continue;
            }

            self.last_error = format!("Checkout 跳到未知頁面: {}", next_url);
            return Ok(false);
        }

        self.last_error = "結帳超過最大步驟數".to_string();
        Ok(false)
    }

    fn watch_request_gap_seconds(interval_secs: f64, _target_count: usize) -> f64 {
        interval_secs
    }

    fn watch_target_refresh_seconds(interval_secs: f64, target_count: usize) -> f64 {
        interval_secs * target_count.max(1) as f64
    }

    fn select_game_rows(rows: &[(String, String)], date_keyword: &str) -> Vec<(String, String)> {
        if rows.is_empty() {
            return Vec::new();
        }

        if date_keyword.trim().is_empty() {
            return vec![rows[0].clone()];
        }

        let matched: Vec<_> = rows
            .iter()
            .filter(|(date, _)| matches_keyword(date, date_keyword))
            .cloned()
            .collect();
        if matched.is_empty() {
            warn!(
                "找不到符合 date_keyword='{}' 的場次，回退第一個可用場次",
                date_keyword
            );
            vec![rows[0].clone()]
        } else {
            matched
        }
    }

    fn select_area(&self, available: &[crate::parser::AreaEntry]) -> crate::parser::AreaEntry {
        if !self.event.area_keyword.trim().is_empty() {
            if let Some(found) = available
                .iter()
                .find(|area| matches_keyword(&area.text, &self.event.area_keyword))
            {
                return found.clone();
            }
        }
        available[0].clone()
    }

    fn filter_selectable_areas(
        areas: Vec<crate::parser::AreaEntry>,
    ) -> Vec<crate::parser::AreaEntry> {
        areas
            .into_iter()
            .filter(|area| !Self::should_skip_area(&area.text))
            .collect()
    }

    fn should_skip_area(text: &str) -> bool {
        let lower = text.to_lowercase();
        text.contains("身心障礙")
            || text.contains("身障")
            || text.contains("輪椅")
            || lower.contains("wheelchair")
            || text.contains("殘障")
            || text.contains("站區")
            || text.contains("搖滾站")
    }

    fn classify_ticket_page_failure(&self, html: &str, final_url: &str) -> String {
        let lower = html.to_lowercase();
        if lower.contains("sold out") || html.contains("已售完") {
            "此區已售完".to_string()
        } else if detect_login_required(html, final_url) {
            "cookie 過期，需要重新登入".to_string()
        } else if html.contains("Browsing Activity") || lower.contains("unusual behavior") {
            "IP 被封鎖 (Browsing Activity Paused)".to_string()
        } else if html.contains("cf-browser-verification") || html.contains("challenge-platform") {
            "Cloudflare challenge".to_string()
        } else if final_url.contains("/ticket/area/") {
            "被踢回選區頁（非票頁）".to_string()
        } else {
            format!("未知票頁錯誤（HTML {} bytes）", html.len())
        }
    }

    fn select_ticket_count(options: &[u32], desired_count: u32) -> Option<u32> {
        if options.is_empty() {
            return Some(desired_count);
        }
        if options.contains(&desired_count) {
            return Some(desired_count);
        }

        options.iter().copied().filter(|value| *value > 0).max()
    }

    fn extract_error_message(html: &str) -> Option<String> {
        let error_re = Regex::new(
            r#"(?is)class=["'](?:help-block|alert|error|warning)["'][^>]*>(.*?)</(?:div|span)>"#,
        )
        .unwrap();
        error_re
            .captures(html)
            .and_then(|caps| caps.get(1).map(|m| m.as_str()))
            .map(|raw| raw.replace('\n', " "))
            .map(|raw| {
                Regex::new(r#"(?is)<[^>]+>"#)
                    .unwrap()
                    .replace_all(&raw, " ")
                    .to_string()
            })
            .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|text| !text.is_empty())
    }

    fn build_form_payload(fields: &std::collections::HashMap<String, String>) -> Vec<(&str, &str)> {
        let mut items = fields.iter().collect::<Vec<_>>();
        items.sort_by(|a, b| a.0.cmp(b.0));
        items
            .into_iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    fn resolve_cookie_file(config: &AppConfig, session: &SessionConfig) -> Option<String> {
        if !session.cookie_file.trim().is_empty() {
            return Some(session.cookie_file.clone());
        }
        if config.sessions.len() > 1 && session.name != "default" {
            return None;
        }
        Some(
            config
                .base_dir
                .join("tixcraft_cookies.json")
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn normalize_url(url: &str) -> String {
        if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else if url.starts_with('/') {
            format!("{BASE_URL}{url}")
        } else {
            format!("{BASE_URL}/{url}")
        }
    }

    fn is_verify_url(url: &str) -> bool {
        url.contains("/activity/verify/") || url.contains("/ticket/verify/")
    }

    fn verify_check_url(verify_url: &str) -> String {
        if verify_url.contains("/activity/verify/") {
            verify_url.replace("/activity/verify/", "/activity/check-code/")
        } else if verify_url.contains("/ticket/verify/") {
            verify_url.replace("/ticket/verify/", "/ticket/check-code/")
        } else {
            verify_url.to_string()
        }
    }

    fn is_checkout_url(url: &str) -> bool {
        url.contains("/ticket/order") || url.contains("/ticket/checkout")
    }

    fn is_success_order_url(url: &str) -> bool {
        url.contains("/order") && !url.contains("/ticket/")
    }

    fn is_fail_url(url: &str) -> bool {
        FAIL_PATTERNS.iter().any(|pattern| url.contains(pattern))
            || url.trim_end_matches('/').ends_with("tixcraft.com")
    }

    fn game_url(&self) -> String {
        let url = &self.event.url;
        if url.contains("/activity/detail/") {
            let slug = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
            format!("{BASE_URL}/activity/game/{slug}")
        } else {
            url.clone()
        }
    }

    fn rebuild_http_client(&mut self, reason: &str) -> Result<()> {
        let previous_proxy = self.proxy.clone();
        let next_proxy = self.proxy_pool.resolve_for_session(&self.session);
        let http = ApiHttpClient::new(next_proxy.as_deref(), &self.cookie_entries)
            .with_context(|| format!("failed to rebuild HTTP client after {}", reason))?;
        self.http = http;
        self.proxy = next_proxy;
        warn!(
            reason = reason,
            old_proxy = previous_proxy.as_deref().unwrap_or("-"),
            new_proxy = self.proxy.as_deref().unwrap_or("-"),
            "HTTP client 已重建"
        );
        Ok(())
    }

    fn is_blocked_error_message(message: &str) -> bool {
        BLOCKED_CODES
            .iter()
            .any(|status| message.contains(&format!("收到 {}", status)))
    }
}

#[cfg(test)]
mod tests {
    use super::TixcraftApiBot;
    use crate::config::{AppConfig, SessionConfig};

    #[test]
    fn resolve_cookie_file_skips_global_file_for_named_multi_session() {
        let config = AppConfig {
            sessions: vec![
                SessionConfig {
                    name: "default".to_string(),
                    ..SessionConfig::default()
                },
                SessionConfig {
                    name: "alt".to_string(),
                    ..SessionConfig::default()
                },
            ],
            ..AppConfig::default()
        };

        let resolved = TixcraftApiBot::resolve_cookie_file(&config, &config.sessions[1]);
        assert_eq!(resolved, None);
    }

    #[test]
    fn verify_check_url_maps_verify_endpoints() {
        assert_eq!(
            TixcraftApiBot::verify_check_url("https://tixcraft.com/activity/verify/26_ive/123"),
            "https://tixcraft.com/activity/check-code/26_ive/123"
        );
        assert_eq!(
            TixcraftApiBot::verify_check_url("https://tixcraft.com/ticket/verify/26_ive/123"),
            "https://tixcraft.com/ticket/check-code/26_ive/123"
        );
    }

    #[test]
    fn select_ticket_count_falls_back_to_highest_positive_option() {
        assert_eq!(TixcraftApiBot::select_ticket_count(&[0, 1, 3], 2), Some(3));
    }

    #[test]
    fn fail_url_matches_game_and_root_pages() {
        assert!(TixcraftApiBot::is_fail_url(
            "https://tixcraft.com/activity/game/26_ive"
        ));
        assert!(TixcraftApiBot::is_fail_url("https://tixcraft.com"));
        assert!(!TixcraftApiBot::is_fail_url(
            "https://tixcraft.com/ticket/order/abc"
        ));
    }

    #[test]
    fn watch_target_refresh_scales_with_target_count() {
        assert_eq!(TixcraftApiBot::watch_request_gap_seconds(3.0, 2), 3.0);
        assert_eq!(TixcraftApiBot::watch_target_refresh_seconds(3.0, 1), 3.0);
        assert_eq!(TixcraftApiBot::watch_target_refresh_seconds(3.0, 2), 6.0);
        assert_eq!(TixcraftApiBot::watch_target_refresh_seconds(3.0, 3), 9.0);
    }

    #[test]
    fn select_game_rows_matches_multi_keyword_dates() {
        let rows = vec![
            (
                "2026/09/11 (Fri) 19:30".to_string(),
                "/ticket/area/11".to_string(),
            ),
            (
                "2026/09/12 (Sat) 19:30".to_string(),
                "/ticket/area/12".to_string(),
            ),
            (
                "2026/09/13 (Sun) 19:30".to_string(),
                "/ticket/area/13".to_string(),
            ),
        ];

        let selected = TixcraftApiBot::select_game_rows(&rows, "2026/09/11|2026/09/13");
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].0, "2026/09/11 (Fri) 19:30");
        assert_eq!(selected[1].0, "2026/09/13 (Sun) 19:30");
    }

    #[test]
    fn select_game_rows_falls_back_to_first_when_keyword_misses() {
        let rows = vec![
            ("2026/09/11".to_string(), "/ticket/area/11".to_string()),
            ("2026/09/12".to_string(), "/ticket/area/12".to_string()),
        ];

        let selected = TixcraftApiBot::select_game_rows(&rows, "2026/10/01");
        assert_eq!(selected, vec![rows[0].clone()]);
    }
}
