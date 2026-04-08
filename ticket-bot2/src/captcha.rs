use crate::config::CaptchaConfig;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{info, warn};
use uuid::Uuid;

pub struct CaptchaSolverBridge {
    python_executable: PathBuf,
    helper_script: PathBuf,
    config: CaptchaConfig,
}

impl CaptchaSolverBridge {
    pub fn new(config: CaptchaConfig) -> Self {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(manifest_dir.clone());

        Self {
            python_executable: repo_root.join(".venv/bin/python"),
            helper_script: manifest_dir.join("scripts/solve_captcha.py"),
            config,
        }
    }

    pub async fn solve(&self, image_bytes: &[u8]) -> Result<String> {
        let tmp_path =
            std::env::temp_dir().join(format!("ticket-bot2-captcha-{}.png", Uuid::new_v4()));
        std::fs::write(&tmp_path, image_bytes)
            .with_context(|| format!("failed to write temp captcha: {}", tmp_path.display()))?;

        let mut command = Command::new(&self.python_executable);
        command
            .arg(&self.helper_script)
            .arg("--image")
            .arg(&tmp_path);

        if self.config.beta_model {
            command.arg("--beta");
        }
        if !self.config.custom_model_path.trim().is_empty() {
            command
                .arg("--custom-model")
                .arg(&self.config.custom_model_path);
        }
        if !self.config.custom_charset_path.trim().is_empty() {
            command
                .arg("--custom-charset")
                .arg(&self.config.custom_charset_path);
        }
        if self.config.char_ranges > 0 {
            command
                .arg("--char-ranges")
                .arg(self.config.char_ranges.to_string());
        }

        let output = command.output().await.with_context(|| {
            format!(
                "failed to run captcha helper: {}",
                self.python_executable.display()
            )
        })?;

        let _ = std::fs::remove_file(&tmp_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("captcha helper failed: {}", stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut best_text = String::new();
        let mut best_conf = 0.0_f32;

        for line in stdout.lines() {
            if let Some(value) = line.strip_prefix("text=") {
                best_text = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("confidence=") {
                best_conf = value.trim().parse::<f32>().unwrap_or(0.0);
            }
        }

        if best_text.len() == 4 {
            info!("captcha solved by helper: {} ({:.2})", best_text, best_conf);
            return Ok(best_text);
        }

        warn!("captcha helper returned invalid result: {}", stdout.trim());
        Ok(best_text)
    }
}
