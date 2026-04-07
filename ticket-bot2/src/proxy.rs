use crate::config::{ProxyConfig, SessionConfig};
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

#[derive(Debug)]
pub struct ProxyPool {
    config: ProxyConfig,
    next_index: AtomicUsize,
}

impl ProxyPool {
    pub fn new(config: ProxyConfig) -> Self {
        Self {
            config,
            next_index: AtomicUsize::new(0),
        }
    }

    pub fn available(&self) -> bool {
        self.config.enabled && !self.config.servers.is_empty()
    }

    pub fn next(&self) -> Option<String> {
        if !self.available() {
            return None;
        }

        let selected = if self.config.rotate {
            let index = self.next_index.fetch_add(1, Ordering::Relaxed);
            self.config
                .servers
                .get(index % self.config.servers.len())
                .cloned()
        } else {
            self.config.servers.first().cloned()
        }?;

        Some(expand_session_id(selected))
    }

    pub fn resolve_for_session(&self, session: &SessionConfig) -> Option<String> {
        if !session.proxy_server.trim().is_empty() {
            return Some(expand_session_id(session.proxy_server.clone()));
        }
        self.next()
    }
}

fn expand_session_id(server: String) -> String {
    if !server.contains("{session_id}") {
        return server;
    }

    let session_id = Uuid::new_v4().simple().to_string();
    server.replace("{session_id}", &session_id[..8])
}

#[cfg(test)]
mod tests {
    use super::ProxyPool;
    use crate::config::{ProxyConfig, SessionConfig};

    #[test]
    fn round_robin_proxy_pool() {
        let pool = ProxyPool::new(ProxyConfig {
            enabled: true,
            rotate: true,
            servers: vec!["http://a:80".into(), "http://b:80".into()],
        });

        assert_eq!(pool.next().as_deref(), Some("http://a:80"));
        assert_eq!(pool.next().as_deref(), Some("http://b:80"));
        assert_eq!(pool.next().as_deref(), Some("http://a:80"));
    }

    #[test]
    fn session_override_wins() {
        let pool = ProxyPool::new(ProxyConfig {
            enabled: true,
            rotate: true,
            servers: vec!["http://a:80".into()],
        });

        let session = SessionConfig {
            proxy_server: "http://fixed:8080".into(),
            ..SessionConfig::default()
        };

        assert_eq!(
            pool.resolve_for_session(&session).as_deref(),
            Some("http://fixed:8080")
        );
    }
}
