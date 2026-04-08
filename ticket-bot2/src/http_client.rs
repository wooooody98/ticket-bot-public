use crate::cookies::CookieEntry;
use anyhow::Result;
use rquest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONNECTION, REFERER,
};
use rquest::Impersonate;
use std::net::IpAddr;

pub const DEFAULT_API_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 ",
    "(KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
);

#[derive(Debug, Clone)]
pub struct ApiHttpClient {
    client: rquest::Client,
    user_agent: String,
}

#[derive(Debug, Clone)]
pub struct ApiTextResponse {
    pub status: u16,
    pub final_url: String,
    pub location: Option<String>,
    pub content_type: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct ApiBinaryResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

impl ApiHttpClient {
    pub fn new(proxy: Option<&str>, cookies: &[CookieEntry]) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
            ),
        );
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("zh-TW,zh;q=0.9,en-US;q=0.8,en;q=0.7"),
        );
        headers.insert(CONNECTION, HeaderValue::from_static("keep-alive"));

        let mut builder = rquest::Client::builder()
            .impersonate(Impersonate::Chrome133)
            .default_headers(headers)
            .cookie_store(true)
            .redirect(rquest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15));

        if let Some(proxy) = proxy {
            builder = builder.proxy(rquest::Proxy::all(proxy)?);
        }

        let client = builder.build()?;
        Self::seed_cookies(&client, cookies)?;

        Ok(Self {
            client,
            user_agent: DEFAULT_API_USER_AGENT.to_string(),
        })
    }

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    pub async fn get_text(&self, url: &str, referer: Option<&str>) -> Result<ApiTextResponse> {
        self.get_text_with_options(url, referer, false).await
    }

    pub async fn get_text_following_redirects(
        &self,
        url: &str,
        referer: Option<&str>,
    ) -> Result<ApiTextResponse> {
        self.get_text_with_options(url, referer, true).await
    }

    pub async fn get_binary(
        &self,
        url: &str,
        query: &[(&str, String)],
        referer: Option<&str>,
    ) -> Result<ApiBinaryResponse> {
        let mut request = self.client.get(url).query(query);
        if let Some(referer) = referer {
            request = request.header(REFERER, referer);
        }
        let response = request.send().await?;
        Self::read_binary_response(&self.client, response).await
    }

    pub async fn post_form(
        &self,
        url: &str,
        referer: Option<&str>,
        form: &[(&str, &str)],
    ) -> Result<ApiTextResponse> {
        let mut request = self.client.post(url).form(form);
        if let Some(referer) = referer {
            request = request.header(REFERER, referer);
        }
        let response = request.send().await?;
        Self::read_response(&self.client, response).await
    }

    fn seed_cookies(client: &rquest::Client, cookies: &[CookieEntry]) -> Result<()> {
        for cookie in cookies {
            let url = Self::cookie_seed_url(cookie)?;
            let header = Self::cookie_header(cookie)?;
            client.set_cookies(&url, vec![header]);
        }
        Ok(())
    }

    fn cookie_seed_url(cookie: &CookieEntry) -> Result<rquest::Url> {
        let host = cookie.domain.trim().trim_start_matches('.');
        let host = if host.is_empty() { "tixcraft.com" } else { host };
        let path = Self::normalize_cookie_path(&cookie.path);
        Ok(rquest::Url::parse(&format!("https://{host}{path}"))?)
    }

    fn cookie_header(cookie: &CookieEntry) -> Result<HeaderValue> {
        let path = Self::normalize_cookie_path(&cookie.path);
        let mut raw = format!("{}={}; Path={path}", cookie.name, cookie.value);
        if Self::should_include_cookie_domain(&cookie.domain) {
            raw.push_str("; Domain=");
            raw.push_str(cookie.domain.trim());
        }
        Ok(HeaderValue::from_str(&raw)?)
    }

    fn normalize_cookie_path(path: &str) -> &str {
        let trimmed = path.trim();
        if trimmed.is_empty() { "/" } else { trimmed }
    }

    fn should_include_cookie_domain(domain: &str) -> bool {
        let host = domain.trim().trim_start_matches('.');
        !host.is_empty() && host != "localhost" && host.parse::<IpAddr>().is_err()
    }

    async fn get_text_with_options(
        &self,
        url: &str,
        referer: Option<&str>,
        follow_redirects: bool,
    ) -> Result<ApiTextResponse> {
        let mut current_url = url.to_string();
        let mut current_referer = referer.map(str::to_owned);

        for redirect_idx in 0..=10 {
            let mut request = self.client.get(&current_url);
            if let Some(referer) = current_referer.as_deref() {
                request = request.header(REFERER, referer);
            }

            let response = request.send().await?;
            let parsed = Self::read_response(&self.client, response).await?;

            if !follow_redirects || !Self::should_follow_redirect(parsed.status) {
                return Ok(parsed);
            }

            let Some(location) = parsed.location.as_deref() else {
                return Ok(parsed);
            };
            if redirect_idx == 10 {
                anyhow::bail!("too many redirects while fetching {}", url);
            }

            current_referer = Some(parsed.final_url.clone());
            current_url = Self::resolve_redirect_url(&parsed.final_url, location)?;
        }

        unreachable!()
    }

    fn should_follow_redirect(status: u16) -> bool {
        matches!(status, 301 | 302 | 303 | 307 | 308)
    }

    fn resolve_redirect_url(base: &str, location: &str) -> Result<String> {
        let base = rquest::Url::parse(base)?;
        Ok(base.join(location)?.to_string())
    }

    fn store_response_cookies(client: &rquest::Client, url: &rquest::Url, headers: &HeaderMap) {
        let cookies = headers
            .get_all("set-cookie")
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        if !cookies.is_empty() {
            client.set_cookies(url, cookies);
        }
    }

    async fn read_response(client: &rquest::Client, response: rquest::Response) -> Result<ApiTextResponse> {
        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let response_url = response.url().clone();
        let headers = response.headers().clone();
        Self::store_response_cookies(client, &response_url, &headers);
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response.text().await?;

        Ok(ApiTextResponse {
            status,
            final_url,
            location,
            content_type,
            body,
        })
    }

    async fn read_binary_response(
        client: &rquest::Client,
        response: rquest::Response,
    ) -> Result<ApiBinaryResponse> {
        let status = response.status().as_u16();
        let response_url = response.url().clone();
        let headers = response.headers().clone();
        Self::store_response_cookies(client, &response_url, &headers);
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response.bytes().await?.to_vec();

        Ok(ApiBinaryResponse {
            status,
            content_type,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ApiHttpClient;
    use crate::cookies::CookieEntry;
    use rquest::header::{HeaderMap, HeaderValue};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    fn spawn_redirect_server() -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);

        let handle = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();

                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                captured.lock().unwrap().push(request.clone());

                match path {
                    "/start" => {
                        let response = concat!(
                            "HTTP/1.1 302 Found\r\n",
                            "Location: /final\r\n",
                            "Set-Cookie: hop=2; Path=/\r\n",
                            "Content-Length: 0\r\n",
                            "Connection: close\r\n",
                            "\r\n"
                        );
                        stream.write_all(response.as_bytes()).unwrap();
                    }
                    "/final" => {
                        let body = request
                            .lines()
                            .find(|line| line.starts_with("Cookie: "))
                            .unwrap_or("Cookie:");
                        let response = format!(
                            concat!(
                                "HTTP/1.1 200 OK\r\n",
                                "Content-Type: text/plain\r\n",
                                "Content-Length: {}\r\n",
                                "Connection: close\r\n",
                                "\r\n",
                                "{}"
                            ),
                            body.len(),
                            body
                        );
                        stream.write_all(response.as_bytes()).unwrap();
                    }
                    _ => {
                        let response = concat!(
                            "HTTP/1.1 404 Not Found\r\n",
                            "Content-Length: 0\r\n",
                            "Connection: close\r\n",
                            "\r\n"
                        );
                        stream.write_all(response.as_bytes()).unwrap();
                    }
                }
            }
        });

        (format!("http://127.0.0.1:{port}"), requests, handle)
    }

    #[test]
    fn seed_cookies_populates_cookie_store() {
        let client = ApiHttpClient::new(
            None,
            &[CookieEntry {
                name: "seed".to_string(),
                value: "1".to_string(),
                domain: ".tixcraft.com".to_string(),
                path: "/".to_string(),
            }],
        )
        .unwrap();
        let url = rquest::Url::parse("https://tixcraft.com/activity/game/26_test").unwrap();
        let cookies = client.client.get_cookies(&url).unwrap();
        let cookies = cookies.to_str().unwrap();

        assert!(cookies.contains("seed=1"), "cookies={cookies}");
    }

    #[test]
    fn store_response_cookies_updates_cookie_store() {
        let client = ApiHttpClient::new(None, &[]).unwrap();
        let url = rquest::Url::parse("https://tixcraft.com/activity/game/26_test").unwrap();
        let mut headers = HeaderMap::new();
        headers.append(
            "set-cookie",
            HeaderValue::from_static("hop=2; Path=/; Domain=.tixcraft.com"),
        );

        ApiHttpClient::store_response_cookies(&client.client, &url, &headers);

        let cookies = client.client.get_cookies(&url).unwrap();
        let cookies = cookies.to_str().unwrap();
        assert!(cookies.contains("hop=2"), "cookies={cookies}");
    }

    #[tokio::test]
    async fn get_text_following_redirects_updates_cookie_store() {
        let (base_url, requests, handle) = spawn_redirect_server();
        let client = ApiHttpClient::new(None, &[]).unwrap();

        let response = client
            .get_text_following_redirects(&format!("{base_url}/start"), None)
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.final_url, format!("{base_url}/final"));

        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert!(captured[0].contains("GET /start HTTP/1.1"));
        assert!(captured[1].contains("GET /final HTTP/1.1"));
        assert!(
            captured[1].to_ascii_lowercase().contains("cookie: hop=2"),
            "request={}",
            captured[1]
        );
        drop(captured);

        handle.join().unwrap();
    }
}
