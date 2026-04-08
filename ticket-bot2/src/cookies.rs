use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CookieEntry {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default = "default_cookie_path")]
    pub path: String,
}

fn default_cookie_path() -> String {
    "/".to_string()
}

/// 從 tixcraft_cookies.json 載入 cookie，回傳可灌入 cookie jar 的條目
pub fn load_cookies(path: impl AsRef<Path>) -> Result<Vec<CookieEntry>> {
    let path = path.as_ref();
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read cookie file: {}", path.display()))?;
    let entries: Vec<CookieEntry> = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse cookie JSON: {}", path.display()))?;

    Ok(entries
        .into_iter()
        .filter(|cookie| {
            let domain = cookie.domain.trim().to_ascii_lowercase();
            domain.is_empty() || domain.contains("tixcraft")
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{load_cookies, CookieEntry};
    use std::fs;

    #[test]
    fn load_cookies_keeps_tixcraft_and_host_only_entries() {
        let path = std::env::temp_dir().join(format!("ticket-bot2-cookies-{}.json", std::process::id()));
        fs::write(
            &path,
            r#"
[
  {"name":"keep_domain","value":"1","domain":".tixcraft.com","path":"/ticket"},
  {"name":"keep_host_only","value":"2","domain":"","path":"/"},
  {"name":"drop","value":"3","domain":".example.com","path":"/"}
]
"#,
        )
        .unwrap();

        let cookies = load_cookies(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(
            cookies,
            vec![
                CookieEntry {
                    name: "keep_domain".to_string(),
                    value: "1".to_string(),
                    domain: ".tixcraft.com".to_string(),
                    path: "/ticket".to_string(),
                },
                CookieEntry {
                    name: "keep_host_only".to_string(),
                    value: "2".to_string(),
                    domain: String::new(),
                    path: "/".to_string(),
                },
            ]
        );
    }
}
