use regex::Regex;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct VerifyInfo {
    pub answer: Option<String>,
    pub csrf: Option<String>,
    pub form_action: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AreaEntry {
    pub url: String,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct AreaInfo {
    pub available: Vec<AreaEntry>,
    pub sold_out: Vec<String>,
    pub total: usize,
}

#[derive(Debug, Clone, Default)]
pub struct TicketFormInfo {
    pub fields: HashMap<String, String>,
    pub select_name: Option<String>,
    pub select_options: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct OrderFormInfo {
    pub fields: HashMap<String, String>,
}

/// 從 game 頁 HTML 解析場次列表，回傳 (date_text, target_url) pairs
pub fn parse_game_list(html: &str) -> Vec<(String, String)> {
    let row_re = Regex::new(r#"(?is)<tr[^>]*>(.*?)</tr>"#).unwrap();
    let href_re = Regex::new(r#"data-href=["']([^"']+)["']"#).unwrap();
    let mut results = Vec::new();

    for row in row_re.captures_iter(html) {
        let row_html = row.get(1).map(|m| m.as_str()).unwrap_or_default();
        let Some(href) = href_re
            .captures(row_html)
            .and_then(|caps| caps.get(1).map(|m| m.as_str()))
        else {
            continue;
        };

        if !is_game_target_href(href) {
            continue;
        }

        let text = collapse_whitespace(&strip_tags(row_html));
        let url = normalize_tixcraft_url(href);
        results.push((text, url));
    }

    results
}

pub fn parse_verify_page(html: &str) -> VerifyInfo {
    let mut info = VerifyInfo::default();
    let answer_re = Regex::new(r#"【(.+?)】"#).unwrap();

    info.csrf = extract_input_value(html, "_csrf");

    let form_re = Regex::new(r#"(?is)<form\b[^>]*>"#).unwrap();
    for form in form_re.find_iter(html) {
        let attrs = parse_tag_attributes(form.as_str());
        let action = attrs.get("action").cloned();
        if attrs.get("id").map(String::as_str) == Some("form-ticket-verify") {
            info.form_action = action;
            break;
        }
        if info.form_action.is_none()
            && action
                .as_deref()
                .map(|value| value.contains("verify") || value.contains("check-code"))
                .unwrap_or(false)
        {
            info.form_action = action;
        }
    }

    if let Some(zone_pos) = html.find("zone-verify") {
        let end = (zone_pos + 800).min(html.len());
        let zone = html[zone_pos..end].replace("「", "【").replace("」", "】");
        info.answer = answer_re
            .captures(&zone)
            .and_then(|caps| caps.get(1).map(|m| m.as_str().trim().to_string()));
    }

    info
}

/// 從 area 頁 HTML 解析可用區域
pub fn parse_area_list(html: &str) -> AreaInfo {
    let anchor_re = Regex::new(r#"(?is)<a([^>]*)>(.*?)</a>"#).unwrap();
    let id_re = Regex::new(r#"id=["']([^"']+)"#).unwrap();
    let href_re = Regex::new(r#"href=["']([^"']*)"#).unwrap();
    let class_re = Regex::new(r#"class=["']([^"']*)"#).unwrap();
    let mut area_urls = parse_area_url_list(html);

    let mut available = Vec::new();
    let mut sold_out = Vec::new();
    let mut total = 0_usize;

    for anchor in anchor_re.captures_iter(html) {
        let attrs = anchor.get(1).map(|m| m.as_str()).unwrap_or_default();
        let text_html = anchor.get(2).map(|m| m.as_str()).unwrap_or_default();
        let text = collapse_whitespace(&strip_tags(text_html));
        if text.is_empty() {
            continue;
        }

        let id = id_re
            .captures(attrs)
            .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
            .unwrap_or_default();
        let mut href = href_re
            .captures(attrs)
            .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
            .unwrap_or_default();
        if href.is_empty() && !id.is_empty() {
            href = area_urls.remove(&id).unwrap_or_default();
        }

        if href.is_empty() && id.is_empty() {
            continue;
        }

        if !href.is_empty() && !is_area_target_href(&href) {
            continue;
        }

        total += 1;
        let class_text = class_re
            .captures(attrs)
            .and_then(|caps| caps.get(1).map(|m| m.as_str().to_lowercase()))
            .unwrap_or_default();
        let is_disabled =
            class_text.contains("disabled") || is_sold_out_text(&text) || href.trim().is_empty();

        if is_disabled {
            sold_out.push(text);
            continue;
        }

        available.push(AreaEntry {
            url: normalize_tixcraft_url(&href),
            text,
        });
    }

    AreaInfo {
        available,
        sold_out,
        total,
    }
}

pub fn parse_ticket_form(html: &str) -> TicketFormInfo {
    let input_re = Regex::new(r#"(?is)<input\b[^>]*>"#).unwrap();
    let select_mobile_re =
        Regex::new(r#"(?is)<select[^>]+name=["']([^"']+)["'][^>]*class=["'][^"']*mobile-select[^"']*["'][^>]*>(.*?)</select>"#)
            .unwrap();
    let select_re =
        Regex::new(r#"(?is)<select[^>]+name=["']([^"']+)["'][^>]*>(.*?)</select>"#).unwrap();
    let option_re = Regex::new(r#"value=["'](\d+)["']"#).unwrap();

    let mut fields = HashMap::new();
    for input in input_re.find_iter(html) {
        let attrs = parse_tag_attributes(input.as_str());
        let Some(input_type) = attrs.get("type") else {
            continue;
        };
        if !input_type.eq_ignore_ascii_case("hidden") {
            continue;
        }
        if let Some(name) = attrs.get("name") {
            let value = attrs.get("value").cloned().unwrap_or_default();
            fields.insert(name.clone(), value);
        }
    }

    let select_caps = select_mobile_re
        .captures(html)
        .or_else(|| select_re.captures(html));
    let (select_name, select_options) = if let Some(caps) = select_caps {
        let name = caps.get(1).map(|m| m.as_str().to_string());
        let body = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
        let options = option_re
            .captures_iter(body)
            .filter_map(|caps| caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok()))
            .collect::<Vec<_>>();
        (name, options)
    } else {
        (None, Vec::new())
    };

    fields
        .entry("TicketForm[agree]".to_string())
        .or_insert_with(|| "1".to_string());

    TicketFormInfo {
        fields,
        select_name,
        select_options,
    }
}

pub fn parse_order_form(html: &str) -> Option<OrderFormInfo> {
    if contains_processing_text(html) {
        return None;
    }

    let csrf = extract_input_value(html, "_csrf")?;
    let mut fields = HashMap::from([("_csrf".to_string(), csrf)]);

    let radio_groups = parse_radio_groups(html);
    if radio_groups.is_empty() {
        if contains_radio_inputs(html) {
            return None;
        }
        if contains_checkout_text(html) {
            add_checkbox_fields(html, &mut fields);
            return Some(OrderFormInfo { fields });
        }
        return None;
    }

    for (name, options) in radio_groups {
        let selected = if name.to_lowercase().contains("payment") {
            select_radio_by_keywords(
                &options,
                &[
                    "atm",
                    "虛擬帳號",
                    "轉帳",
                    "匯款",
                    "virtual",
                    "ibon",
                    "超商繳費",
                ],
            )
        } else if name.to_lowercase().contains("shipment")
            || name.to_lowercase().contains("delivery")
        {
            select_radio_by_keywords(&options, &["ibon", "超商", "便利商店", "7-eleven", "7-11"])
        } else {
            options
                .first()
                .map(|opt| opt.value.clone())
                .unwrap_or_default()
        };
        if !selected.is_empty() {
            fields.insert(name, selected);
        }
    }

    add_checkbox_fields(html, &mut fields);
    Some(OrderFormInfo { fields })
}

pub fn detect_coming_soon(html: &str) -> bool {
    let lower = html.to_lowercase();
    lower.contains("coming soon")
        || html.contains("即將開賣")
        || html.contains("尚未開賣")
        || html.contains("即将开卖")
        || html.contains("まもなく販売開始")
}

pub fn detect_login_required(html: &str, url: &str) -> bool {
    let lower_url = url.to_lowercase();
    if lower_url.contains("login")
        || lower_url.contains("facebook.com")
        || lower_url.contains("accounts.google.com")
    {
        return true;
    }

    let lower = html.to_lowercase();
    (lower.contains("login") || lower.contains("sign in") || html.contains("登入"))
        && lower.contains("<form")
        && lower.contains("login")
}

pub fn split_keywords(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }

    raw.split(['|', ',', '\n', ';', '，', '；'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 關鍵字匹配：支援 "|"、逗號、分號、換行分隔多關鍵字
pub fn matches_keyword(text: &str, keyword: &str) -> bool {
    let keywords = split_keywords(keyword);
    if keywords.is_empty() {
        return false;
    }
    keywords.iter().any(|part| text.contains(part))
}

#[derive(Debug, Clone)]
struct RadioOption {
    value: String,
    label: String,
}

fn is_game_target_href(href: &str) -> bool {
    href.starts_with("/ticket/area/")
        || href.starts_with("https://tixcraft.com/ticket/area/")
        || href.starts_with("/activity/verify/")
        || href.starts_with("https://tixcraft.com/activity/verify/")
        || href.starts_with("/ticket/verify/")
        || href.starts_with("https://tixcraft.com/ticket/verify/")
}

fn normalize_tixcraft_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with('/') {
        format!("https://tixcraft.com{url}")
    } else {
        format!("https://tixcraft.com/{url}")
    }
}

fn is_area_target_href(href: &str) -> bool {
    href.starts_with("/ticket/ticket/")
        || href.starts_with("https://tixcraft.com/ticket/ticket/")
        || href.starts_with("/ticket/verify/")
        || href.starts_with("https://tixcraft.com/ticket/verify/")
        || href.starts_with("/activity/verify/")
        || href.starts_with("https://tixcraft.com/activity/verify/")
}

fn contains_processing_text(html: &str) -> bool {
    let lower = html.to_lowercase();
    lower.contains("sit tight")
        || lower.contains("securing your")
        || html.contains("請稍候")
        || html.contains("處理中")
        || lower.contains("processing")
}

fn contains_checkout_text(html: &str) -> bool {
    let lower = html.to_lowercase();
    lower.contains("checkout")
        || html.contains("結帳")
        || html.contains("確認付款")
        || html.contains("確認")
        || html.contains("送出")
}

fn is_sold_out_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    text.contains("選購一空")
        || text.contains("已售完")
        || lower.contains("sold out")
        || lower.contains("no tickets")
        || text.contains("空席なし")
        || text.contains("完売")
        || text.contains("暫無")
}

fn parse_area_url_list(html: &str) -> HashMap<String, String> {
    let block_re = Regex::new(r#"(?is)areaUrlList\s*=\s*\{(.*?)\};"#).unwrap();
    let pair_re = Regex::new(r#"['"]?([^'":,{}\s]+)['"]?\s*:\s*['"]([^"']+)['"]"#).unwrap();
    let mut result = HashMap::new();

    let Some(block) = block_re
        .captures(html)
        .and_then(|caps| caps.get(1).map(|m| m.as_str()))
    else {
        return result;
    };

    for pair in pair_re.captures_iter(block) {
        let key = pair.get(1).map(|m| m.as_str()).unwrap_or_default();
        let value = pair.get(2).map(|m| m.as_str()).unwrap_or_default();
        if !key.is_empty() && !value.is_empty() {
            result.insert(key.to_string(), value.to_string());
        }
    }

    result
}

fn parse_radio_groups(html: &str) -> HashMap<String, Vec<RadioOption>> {
    let mut groups: HashMap<String, Vec<RadioOption>> = HashMap::new();
    let input_re = Regex::new(r#"(?is)<input\b[^>]*>"#).unwrap();

    for input in input_re.find_iter(html) {
        let attrs = parse_tag_attributes(input.as_str());
        let Some(input_type) = attrs.get("type") else {
            continue;
        };
        if !input_type.eq_ignore_ascii_case("radio") {
            continue;
        }

        let Some(name) = attrs.get("name").cloned() else {
            continue;
        };
        let Some(value) = attrs.get("value").cloned() else {
            continue;
        };

        let label = extract_label_after(html, input.end());
        let options = groups.entry(name).or_default();
        if !options.iter().any(|opt| opt.value == value) {
            options.push(RadioOption { value, label });
        }
    }

    groups
}

fn extract_label_after(html: &str, start: usize) -> String {
    let end = (start + 300).min(html.len());
    let after = &html[start..end];
    let label_slice = after.split("</label>").next().unwrap_or(after);
    collapse_whitespace(&strip_tags(label_slice)).to_lowercase()
}

fn select_radio_by_keywords(options: &[RadioOption], keywords: &[&str]) -> String {
    for keyword in keywords {
        for option in options {
            if option.label.contains(&keyword.to_lowercase()) {
                return option.value.clone();
            }
        }
    }
    options
        .first()
        .map(|opt| opt.value.clone())
        .unwrap_or_default()
}

fn add_checkbox_fields(html: &str, fields: &mut HashMap<String, String>) {
    let input_re = Regex::new(r#"(?is)<input\b[^>]*>"#).unwrap();
    for input in input_re.find_iter(html) {
        let attrs = parse_tag_attributes(input.as_str());
        let Some(input_type) = attrs.get("type") else {
            continue;
        };
        if !input_type.eq_ignore_ascii_case("checkbox") {
            continue;
        }
        if let Some(name) = attrs.get("name") {
            fields.insert(name.clone(), "1".to_string());
        }
    }
}

fn contains_radio_inputs(html: &str) -> bool {
    let input_re = Regex::new(r#"(?is)<input\b[^>]*>"#).unwrap();
    let has_radio = input_re.find_iter(html).any(|input| {
        parse_tag_attributes(input.as_str())
            .get("type")
            .map(|value| value.eq_ignore_ascii_case("radio"))
            .unwrap_or(false)
    });
    has_radio
}

fn extract_input_value(html: &str, target_name: &str) -> Option<String> {
    let input_re = Regex::new(r#"(?is)<input\b[^>]*>"#).unwrap();
    for input in input_re.find_iter(html) {
        let attrs = parse_tag_attributes(input.as_str());
        if attrs.get("name").map(String::as_str) == Some(target_name) {
            return Some(attrs.get("value").cloned().unwrap_or_default());
        }
    }
    None
}

fn parse_tag_attributes(tag: &str) -> HashMap<String, String> {
    let attr_re =
        Regex::new(r#"([A-Za-z_:][-A-Za-z0-9_:.]*)\s*=\s*["']([^"']*)["']"#).unwrap();
    let mut attrs = HashMap::new();
    for caps in attr_re.captures_iter(tag) {
        let Some(name) = caps.get(1).map(|m| m.as_str().to_ascii_lowercase()) else {
            continue;
        };
        let value = caps
            .get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        attrs.insert(name, value);
    }
    attrs
}

fn strip_tags(html: &str) -> String {
    let tag_re = Regex::new(r#"(?is)<[^>]+>"#).unwrap();
    tag_re.replace_all(html, " ").to_string()
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAME_PAGE_FIXTURE: &str = include_str!("../tests/fixtures/game_page.html");
    const VERIFY_PAGE_FIXTURE: &str = include_str!("../tests/fixtures/verify_page.html");
    const AREA_PAGE_FIXTURE: &str = include_str!("../tests/fixtures/area_page.html");
    const TICKET_PAGE_FIXTURE: &str = include_str!("../tests/fixtures/ticket_page.html");
    const ORDER_PAGE_FIXTURE: &str = include_str!("../tests/fixtures/order_page.html");

    #[test]
    fn test_matches_keyword() {
        assert!(matches_keyword("2026/09/11 (五) 19:00", "2026/09/11"));
        assert!(matches_keyword(
            "2026/09/11 (五) 19:00",
            "2026/09/11|2026/09/12"
        ));
        assert!(matches_keyword(
            "2026/09/11 (五) 19:00",
            "2026/09/10, 2026/09/11"
        ));
        assert!(!matches_keyword(
            "2026/09/13 (日) 19:00",
            "2026/09/11|2026/09/12"
        ));
        assert!(!matches_keyword("anything", ""));
    }

    #[test]
    fn test_split_keywords_supports_multiple_delimiters() {
        assert_eq!(
            split_keywords("2026/09/11|2026/09/12,2026/09/13；2026/09/14\n2026/09/15"),
            vec![
                "2026/09/11",
                "2026/09/12",
                "2026/09/13",
                "2026/09/14",
                "2026/09/15",
            ]
        );
    }

    #[test]
    fn test_parse_game_list_keeps_verify_targets() {
        let html = r#"
            <tr>
              <td>2026/09/11 (五) 19:00</td>
              <td><button data-href="/activity/verify/26_ive/12345">立即購票</button></td>
            </tr>
        "#;

        let rows = parse_game_list(html);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "2026/09/11 (五) 19:00 立即購票");
        assert_eq!(
            rows[0].1,
            "https://tixcraft.com/activity/verify/26_ive/12345"
        );
    }

    #[test]
    fn test_parse_verify_page_extracts_answer_and_csrf() {
        let html = r#"
            <form id="form-ticket-verify" action="/activity/check-code/26_ive/12345">
              <input type="hidden" name="_csrf" value="csrf-token">
              <div class="zone-verify">請輸入【ABCD】後繼續</div>
            </form>
        "#;

        let info = parse_verify_page(html);
        assert_eq!(info.answer.as_deref(), Some("ABCD"));
        assert_eq!(info.csrf.as_deref(), Some("csrf-token"));
        assert_eq!(
            info.form_action.as_deref(),
            Some("/activity/check-code/26_ive/12345")
        );
    }

    #[test]
    fn test_parse_verify_page_from_fixture() {
        let info = parse_verify_page(VERIFY_PAGE_FIXTURE);
        assert_eq!(info.answer.as_deref(), Some("ABCD"));
        assert_eq!(info.csrf.as_deref(), Some("csrf-demo-token"));
        assert_eq!(
            info.form_action.as_deref(),
            Some("/activity/check-code/26_demo/1001")
        );
    }

    #[test]
    fn test_parse_area_list_reads_area_url_map() {
        let html = r#"
            <script>
              var areaUrlList = {'A1': '/ticket/ticket/26_ive/111'};
            </script>
            <div class="zone">
              <a id="A1">搖滾A區</a>
              <a class="disabled">已售完區</a>
            </div>
        "#;

        let info = parse_area_list(html);
        assert_eq!(info.total, 1);
        assert_eq!(info.available.len(), 1);
        assert_eq!(
            info.available[0].url,
            "https://tixcraft.com/ticket/ticket/26_ive/111"
        );
        assert_eq!(info.available[0].text, "搖滾A區");
    }

    #[test]
    fn test_parse_area_list_skips_unrelated_links() {
        let html = r#"
            <a href="/news/detail/1101">請慎防詐騙，切勿相信來路不明的客服通知</a>
            <script>
              var areaUrlList = {'A1': '/ticket/ticket/26_ive/111'};
            </script>
            <div class="zone">
              <a id="A1">紫2A區</a>
            </div>
        "#;

        let info = parse_area_list(html);
        assert_eq!(info.total, 1);
        assert_eq!(info.available.len(), 1);
        assert_eq!(info.available[0].text, "紫2A區");
        assert_eq!(
            info.available[0].url,
            "https://tixcraft.com/ticket/ticket/26_ive/111"
        );
    }

    #[test]
    fn test_parse_game_list_from_fixture() {
        let rows = parse_game_list(GAME_PAGE_FIXTURE);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "2026/09/11 (五) 19:30 立即購票");
        assert_eq!(
            rows[0].1,
            "https://tixcraft.com/activity/verify/26_demo/1001"
        );
        assert_eq!(
            rows[1].1,
            "https://tixcraft.com/ticket/area/26_demo/1002"
        );
    }

    #[test]
    fn test_parse_area_list_from_fixture() {
        let info = parse_area_list(AREA_PAGE_FIXTURE);
        assert_eq!(info.total, 2);
        assert_eq!(info.available.len(), 1);
        assert_eq!(info.sold_out, vec!["已售完 紫2B區"]);
        assert_eq!(info.available[0].text, "紫2A區");
        assert_eq!(
            info.available[0].url,
            "https://tixcraft.com/ticket/ticket/26_demo/2001"
        );
    }

    #[test]
    fn test_parse_ticket_form_extracts_fields_and_options() {
        let html = r#"
            <form>
              <input type="hidden" name="_csrf" value="token">
              <select name="TicketForm[ticketPrice][01]" class="mobile-select">
                <option value="0">0</option>
                <option value="2">2</option>
              </select>
            </form>
        "#;

        let info = parse_ticket_form(html);
        assert_eq!(info.fields.get("_csrf").map(String::as_str), Some("token"));
        assert_eq!(
            info.select_name.as_deref(),
            Some("TicketForm[ticketPrice][01]")
        );
        assert_eq!(info.select_options, vec![0, 2]);
        assert_eq!(
            info.fields.get("TicketForm[agree]").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn test_parse_ticket_form_from_fixture() {
        let info = parse_ticket_form(TICKET_PAGE_FIXTURE);
        assert_eq!(
            info.fields.get("_csrf").map(String::as_str),
            Some("csrf-ticket-token")
        );
        assert_eq!(
            info.fields.get("TicketForm[eventId]").map(String::as_str),
            Some("evt-001")
        );
        assert_eq!(
            info.select_name.as_deref(),
            Some("TicketForm[ticketPrice][01]")
        );
        assert_eq!(info.select_options, vec![0, 1, 2]);
    }

    #[test]
    fn test_parse_ticket_form_handles_reordered_hidden_attributes() {
        let html = r#"
            <form>
              <input value="token" name="_csrf" type="hidden">
            </form>
        "#;

        let info = parse_ticket_form(html);
        assert_eq!(info.fields.get("_csrf").map(String::as_str), Some("token"));
    }

    #[test]
    fn test_parse_order_form_selects_payment_and_shipment() {
        let html = r#"
            <input type="hidden" name="_csrf" value="csrf">
            <label><input type="radio" name="CheckoutForm[paymentId]" value="11"> 信用卡 </label>
            <label><input type="radio" name="CheckoutForm[paymentId]" value="12"> ATM 轉帳 </label>
            <label><input type="radio" name="CheckoutForm[shipmentId]" value="21"> 郵寄 </label>
            <label><input type="radio" name="CheckoutForm[shipmentId]" value="22"> ibon 取票 </label>
            <input type="checkbox" name="CheckoutForm[agree]">
        "#;

        let form = parse_order_form(html).expect("form should parse");
        assert_eq!(
            form.fields
                .get("CheckoutForm[paymentId]")
                .map(String::as_str),
            Some("12")
        );
        assert_eq!(
            form.fields
                .get("CheckoutForm[shipmentId]")
                .map(String::as_str),
            Some("22")
        );
        assert_eq!(
            form.fields.get("CheckoutForm[agree]").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn test_parse_order_form_from_fixture() {
        let form = parse_order_form(ORDER_PAGE_FIXTURE).expect("form should parse");
        assert_eq!(
            form.fields.get("_csrf").map(String::as_str),
            Some("csrf-order-token")
        );
        assert_eq!(
            form.fields
                .get("CheckoutForm[paymentId]")
                .map(String::as_str),
            Some("12")
        );
        assert_eq!(
            form.fields
                .get("CheckoutForm[shipmentId]")
                .map(String::as_str),
            Some("22")
        );
        assert_eq!(
            form.fields.get("CheckoutForm[agree]").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn test_parse_order_form_handles_any_radio_attribute_order() {
        let html = r#"
            <input value="csrf" name="_csrf" type="hidden">
            <label><input name="CheckoutForm[paymentId]" type="radio" value="11"> 信用卡 </label>
            <label><input value="22" type="radio" name="CheckoutForm[shipmentId]"> ibon 取票 </label>
            <input name="CheckoutForm[agree]" type="checkbox">
        "#;

        let form = parse_order_form(html).expect("form should parse");
        assert_eq!(
            form.fields
                .get("CheckoutForm[paymentId]")
                .map(String::as_str),
            Some("11")
        );
        assert_eq!(
            form.fields
                .get("CheckoutForm[shipmentId]")
                .map(String::as_str),
            Some("22")
        );
        assert_eq!(
            form.fields.get("CheckoutForm[agree]").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn test_parse_order_form_rejects_unparseable_radio_checkout() {
        let html = r#"
            <input type="hidden" name="_csrf" value="csrf">
            <div>Checkout</div>
            <input type="radio" checked>
        "#;

        assert!(parse_order_form(html).is_none());
    }
}
