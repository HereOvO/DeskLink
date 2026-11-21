//! DeskLink Windows Client.
//!
//! Receives input from the Windows Host and replays it on this machine, and
//! serves the clipboard channel. Together with `desklink-windows` running as
//! the host this gives a Windows -> Windows soft KVM.

mod clipboard;
mod cursor;
mod desktop;
mod inject;
mod keys;

use anyhow::{Context, Result};
use cursor::RemoteCursor;
use desklink_config::DeskLinkConfig;
use desklink_protocol::{decode, encode, token_fingerprint, InputEvent, Packet, PROTOCOL_VERSION};
use inject::Injector;
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing::{info, warn};

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn place_cursor(desktop: &desktop::VirtualDesktop, cursor: &RemoteCursor, dry_run: bool) {
    let (x, y) = cursor.physical_position(desktop);
    if dry_run {
        tracing::debug!(x, y, "dry-run cursor placement");
        return;
    }
    if let Err(code) = desktop::move_cursor_to(x, y) {
        warn!(x, y, code, "failed to move the cursor");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    desktop::enable_dpi_awareness();
    tracing_subscriber::fmt().with_env_filter("info").init();

    let config_path =
        std::env::var("DESKLINK_CONFIG").unwrap_or_else(|_| "desklink.toml".to_owned());
    let config = DeskLinkConfig::load_optional(&config_path)?;
    let bind: SocketAddr = std::env::var("DESKLINK_BIND")
        .unwrap_or_else(|_| config.network.bind.clone())
        .parse()
        .context("invalid DESKLINK_BIND")?;
    let token = std::env::var("DESKLINK_TOKEN").unwrap_or_else(|_| config.security.token.clone());
    let token_id = token_fingerprint(&token);
    let dry_run = env_flag("DESKLINK_DRY_RUN");

    let desktop = desktop::virtual_desktop();

    if config.clipboard.enabled {
        let clipboard_bind = config.network.control_bind.clone();
        let clipboard_token = token.clone();
        let clipboard_config = config.clipboard.clone();
        tokio::spawn(async move {
            if let Err(error) = clipboard::run(
                clipboard_bind,
                clipboard_token,
                clipboard_config.poll_ms,
                clipboard_config.max_bytes,
            )
            .await
            {
                warn!(%error, "clipboard service stopped; input sharing remains available");
            }
        });
    }

    let disconnect_timeout =
        Duration::from_millis(config.performance.disconnect_timeout_ms.max(1_000));
    let socket = UdpSocket::bind(bind).await.context("bind input port")?;
    let mut injector = Injector::new(dry_run);
    let mut cursor = RemoteCursor::new(config.topology.remote_width, config.topology.remote_height);

    if cursor.width != desktop.width || cursor.height != desktop.height {
        warn!(
            remote = %format!("{}x{}", cursor.width, cursor.height),
            desktop = %desktop,
            "host remote-screen box differs from this desktop; set topology.remote_width/height (or drag the box) to this resolution for 1:1 motion"
        );
    }
    if dry_run {
        warn!("dry-run mode: input is decoded and logged but never injected");
    }

    let mut buf = [0u8; 2048];
    let mut remote = false;
    let mut authorized: Option<(SocketAddr, u64)> = None;
    let mut sequence: Option<u32> = None;
    let mut last_seen = Instant::now();
    info!(
        ?bind,
        protocol = PROTOCOL_VERSION,
        config = %config_path,
        token_id = %token_id,
        desktop = %desktop,
        "DeskLink Windows Client listening"
    );

    loop {
        let (n, peer) = match timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await {
            Ok(result) => result?,
            Err(_) => {
                if authorized.is_some() && last_seen.elapsed() > disconnect_timeout {
                    if remote {
                        if let Err(error) = injector.release_all() { warn!(%error, "release-all failed"); }
                    }
                    remote = false;
                    authorized = None;
                    sequence = None;
                    warn!("host timeout; session cleared");
                }
                continue;
            }
        };
        let packet = match decode(&buf[..n]) {
            Ok(packet) => packet,
            Err(error) => {
                warn!(%error, "invalid packet");
                continue;
            }
        };
        match packet {
            Packet::Hello {
                version, session, ..
            } if version != PROTOCOL_VERSION => {
                socket
                    .send_to(
                        &encode(&Packet::Reject {
                            session,
                            reason: format!(
                                "protocol mismatch: host={version}, windows-client={PROTOCOL_VERSION}"
                            ),
                        })?,
                        peer,
                    )
                    .await?;
                warn!(?peer, version, "rejected incompatible host");
            }
            Packet::Hello {
                token: supplied,
                session,
                ..
            } if supplied != token => {
                socket
                    .send_to(
                        &encode(&Packet::Reject {
                            session,
                            reason: "shared token mismatch".to_owned(),
                        })?,
                        peer,
                    )
                    .await?;
                warn!(?peer, "rejected host with wrong token");
            }
            Packet::Hello {
                version,
                token: supplied,
                session,
                ..
            } if version == PROTOCOL_VERSION && supplied == token => {
                if authorized != Some((peer, session)) {
                    if remote {
                        if let Err(error) = injector.release_all() { warn!(%error, "release-all failed"); }
                    }
                    remote = false;
                    sequence = None;
                    authorized = Some((peer, session));
                    info!(?peer, session, "Windows host authorized");
                }
                last_seen = Instant::now();
                socket
                    .send_to(&encode(&Packet::Ack { session })?, peer)
                    .await?;
            }
            Packet::Heartbeat { session, .. } if authorized == Some((peer, session)) => {
                last_seen = Instant::now();
                socket
                    .send_to(&encode(&Packet::Ack { session })?, peer)
                    .await?;
            }
            Packet::Input {
                session,
                sequence: current,
                event,
            } if remote && authorized == Some((peer, session)) => {
                last_seen = Instant::now();
                if sequence
                    .map(|previous| sequence_is_newer(current, previous))
                    .unwrap_or(true)
                {
                    sequence = Some(current);
                    if let InputEvent::MouseMove { dx, dy } = event {
                        if let Some((edge, ratio)) = cursor.move_by(dx, dy) {
                            cursor.snap_to_edge(edge);
                            place_cursor(&desktop, &cursor, dry_run);
                            remote = false;
                            if let Err(error) = injector.release_all() { warn!(%error, "release-all failed"); }
                            socket
                                .send_to(
                                    &encode(&Packet::EdgeReturn {
                                        session,
                                        edge,
                                        ratio,
                                    })?,
                                    peer,
                                )
                                .await?;
                            info!(?edge, ratio, "cursor returned to the host edge");
                            continue;
                        }
                        place_cursor(&desktop, &cursor, dry_run);
                        continue;
                    }
                    if let Err(error) = injector.emit(event) {
                        warn!(%error, "input injection failed");
                    }
                }
            }
            Packet::EnterRemote {
                session,
                edge,
                ratio,
                width,
                height,
            } if authorized == Some((peer, session)) => {
                cursor.enter(edge, ratio, width, height);
                place_cursor(&desktop, &cursor, dry_run);
                remote = true;
                sequence = None;
                last_seen = Instant::now();
                socket
                    .send_to(&encode(&Packet::Ack { session })?, peer)
                    .await?;
                info!(?edge, ratio, width, height, "cursor entered this Windows screen");
            }
            Packet::SetState {
                session,
                remote: requested,
            } if authorized == Some((peer, session)) => {
                last_seen = Instant::now();
                if remote != requested {
                    if !requested {
                        if let Err(error) = injector.release_all() { warn!(%error, "release-all failed"); }
                    }
                    remote = requested;
                    info!(?remote, "input state changed");
                }
                socket
                    .send_to(&encode(&Packet::Ack { session })?, peer)
                    .await?;
            }
            Packet::ReleaseAll { session } if authorized == Some((peer, session)) => {
                last_seen = Instant::now();
                if let Err(error) = injector.release_all() { warn!(%error, "release-all failed"); }
            }
            _ => {}
        }
        if authorized.is_some() && last_seen.elapsed() > disconnect_timeout {
            if remote {
                if let Err(error) = injector.release_all() { warn!(%error, "release-all failed"); }
            }
            remote = false;
            authorized = None;
            sequence = None;
            warn!("host timeout; session cleared");
        }
    }
}

fn sequence_is_newer(sequence: u32, previous: u32) -> bool {
    sequence != previous && sequence.wrapping_sub(previous) < (u32::MAX / 2)
}
