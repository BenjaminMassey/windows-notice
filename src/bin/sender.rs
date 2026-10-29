//! One-shot sender: reads a TOML config, applies CLI overrides, and delivers a
//! single notice to a running receiver.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use base64::Engine;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;
use windows_notice::{Ack, DEFAULT_PORT, Image, Notice};

/// Send a Windows toast notification to a remote receiver.
///
/// Values come from the TOML config file; any flag given here overrides it.
#[derive(Parser)]
struct Args {
    /// Path to the TOML config file. Missing is fine if the flags cover everything.
    #[arg(short, long, default_value = "notice.toml")]
    config: PathBuf,
    /// Receiver address, e.g. `192.168.1.50`, `pc-name:9001`, or `ws://host:port`.
    #[arg(short, long)]
    target: Option<String>,
    #[arg(long)]
    title: Option<String>,
    #[arg(short, long)]
    message: Option<String>,
    /// Path to an image file on this machine.
    #[arg(short, long)]
    image: Option<PathBuf>,
    /// Ignore the image set in the config file.
    #[arg(long, conflicts_with = "image")]
    no_image: bool,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Config {
    target: Option<String>,
    title: Option<String>,
    message: Option<String>,
    image: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let config_path = find_config(&args.config);
    let mut config: Config = match std::fs::read_to_string(&config_path) {
        Ok(text) => toml::from_str(&text)
            .with_context(|| format!("parsing {}", config_path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", config_path.display())),
    };
    // A relative image path in the config means "relative to the config file".
    if let (Some(image), Some(dir)) = (&config.image, config_path.parent()) {
        config.image = Some(dir.join(image));
    }

    let Some(target) = args.target.or(config.target) else {
        bail!("no target: set `target` in the config or pass --target");
    };
    let Some(message) = args.message.or(config.message) else {
        bail!("no message: set `message` in the config or pass --message");
    };
    let image_path = if args.no_image { None } else { args.image.or(config.image) };

    let image = match image_path {
        Some(path) => Some(load_image(&path)?),
        None => None,
    };
    let notice = Notice { title: args.title.or(config.title), message, image };

    let url = normalize_url(&target);
    let (mut ws, _) = tokio_tungstenite::connect_async(url.as_str())
        .await
        .with_context(|| format!("connecting to {url}"))?;

    ws.send(Message::text(serde_json::to_string(&notice)?)).await?;

    // Wait for the receiver to confirm the toast was shown (or report why not).
    let ack: Ack = loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) => break serde_json::from_str(&text)?,
            Some(Ok(Message::Close(_))) | None => bail!("receiver closed the connection without replying"),
            Some(Ok(_)) => continue,
            Some(Err(e)) => return Err(e.into()),
        }
    };
    let _ = ws.close(None).await;

    if !ack.ok {
        bail!("receiver failed to show notice: {}", ack.error.unwrap_or_default());
    }
    println!("Notice delivered to {url}");
    Ok(())
}

/// Use the config path as given if it exists; otherwise, for a relative path,
/// fall back to the same name next to the executable.
fn find_config(path: &Path) -> PathBuf {
    if path.is_relative() && !path.exists() {
        if let Some(beside_exe) = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join(path))) {
            if beside_exe.exists() {
                return beside_exe;
            }
        }
    }
    path.to_path_buf()
}

fn load_image(path: &Path) -> Result<Image> {
    let bytes = std::fs::read(path).with_context(|| format!("reading image {}", path.display()))?;
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .context("image path has no file extension")?;
    Ok(Image { extension, data: base64::engine::general_purpose::STANDARD.encode(bytes) })
}

/// Accept bare hosts and `host:port` as well as full `ws://` URLs.
fn normalize_url(target: &str) -> String {
    let with_scheme = if target.contains("://") { target.to_string() } else { format!("ws://{target}") };
    let authority = with_scheme.split("://").nth(1).unwrap_or_default();
    let host = authority.split('/').next().unwrap_or_default();
    // Add the default port only when none is given; colons inside `[ipv6]` don't count.
    let has_port = match host.rfind(']') {
        Some(i) => host[i..].contains(':'),
        None => host.contains(':'),
    };
    if has_port { with_scheme } else { with_scheme.replacen(host, &format!("{host}:{DEFAULT_PORT}"), 1) }
}
