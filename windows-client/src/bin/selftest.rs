//! End-to-end probe for `desklink-windows-client`.
//!
//! Plays the Windows Host role against a running client:
//! auth / heartbeat / reject / enter / move / edge-return / release / clipboard.
//!
//! Usage: desklink-selftest [udp-addr] [tcp-addr] [token]
//! Defaults: 127.0.0.1:24901, 127.0.0.1:24900, selftest-token

use anyhow::{bail, Context, Result};
use desklink_protocol::{
    decode, decode_control, encode, encode_control, ControlMessage, InputEvent, Packet, ScreenEdge,
    PROTOCOL_VERSION,
};
use std::{
    net::SocketAddr,
    process::ExitCode,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    time::{sleep, timeout},
};

const SESSION: u64 = 0x5EED_1234_5678_9ABC;
const CLIPBOARD_MARKER: &str = "desklink-selftest-clipboard-marker";

struct Harness {
    socket: UdpSocket,
    token: String,
}

impl Harness {
    async fn send(&self, packet: &Packet) -> Result<()> {
        self.socket.send(&encode(packet)?).await?;
        Ok(())
    }

    async fn expect(&self, what: &str) -> Result<Packet> {
        let mut buffer = [0u8; 2048];
        let (n, _) = timeout(Duration::from_secs(3), self.socket.recv_from(&mut buffer))
            .await
            .with_context(|| format!("timed out waiting for {what}"))?
            .context("receive failed")?;
        decode(&buffer[..n]).context("decode reply")
    }

    async fn expect_ack(&self, what: &str) -> Result<()> {
        match self.expect(what).await? {
            Packet::Ack { session } if session == SESSION => Ok(()),
            other => bail!("{what}: expected Ack for session {SESSION}, got {other:?}"),
        }
    }
}

async fn clipboard_check(target: SocketAddr) -> Result<()> {
    let mut stream = TcpStream::connect(target)
        .await
        .with_context(|| format!("connect clipboard port {target}"))?;
    let hello = ControlMessage::Hello {
        version: PROTOCOL_VERSION,
        device_id: "desklink-selftest".to_owned(),
        token: "selftest-token".to_owned(),
    };
    write_frame(&mut stream, &hello).await?;
    match read_frame(&mut stream).await? {
        ControlMessage::Ready => {}
        other => bail!("clipboard handshake: expected Ready, got {other:?}"),
    }

    write_frame(
        &mut stream,
        &ControlMessage::ClipboardText {
            id: 1,
            text: CLIPBOARD_MARKER.to_owned(),
        },
    )
    .await?;
    // No acknowledgement on this channel; the wrapper script inspects the real
    // clipboard afterwards.
    sleep(Duration::from_millis(600)).await;
    Ok(())
}

async fn write_frame(stream: &mut TcpStream, message: &ControlMessage) -> Result<()> {
    let data = encode_control(message)?;
    stream.write_u32(data.len() as u32).await?;
    stream.write_all(&data).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_frame(stream: &mut TcpStream) -> Result<ControlMessage> {
    let length = timeout(Duration::from_secs(3), stream.read_u32())
        .await
        .context("clipboard read timeout")?? as usize;
    if length > 70 * 1024 * 1024 {
        bail!("clipboard frame too large: {length}");
    }
    let mut data = vec![0; length];
    timeout(Duration::from_secs(3), stream.read_exact(&mut data))
        .await
        .context("clipboard payload timeout")??;
    Ok(decode_control(&data)?)
}

async fn run(harness: &Harness) -> Result<()> {
    let mut failures = 0usize;
    macro_rules! step {
        ($name:literal, $body:expr) => {{
            match $body.await {
                Ok(()) => println!("[PASS] {}", $name),
                Err(error) => {
                    failures += 1;
                    println!("[FAIL] {}: {error:#}", $name);
                }
            }
        }};
    }

    step!("hello with correct token is acknowledged", async {
        harness
            .send(&Packet::Hello {
                version: PROTOCOL_VERSION,
                device_id: "desklink-windows-host".to_owned(),
                token: harness.token.clone(),
                session: SESSION,
            })
            .await?;
        harness.expect_ack("ack after hello").await
    });

    step!("heartbeat is acknowledged", async {
        harness
            .send(&Packet::Heartbeat {
                session: SESSION,
                timestamp_ms: 1,
            })
            .await?;
        harness.expect_ack("ack after heartbeat").await
    });

    step!("wrong token is rejected", async {
        harness
            .send(&Packet::Hello {
                version: PROTOCOL_VERSION,
                device_id: "intruder".to_owned(),
                token: "definitely-wrong".to_owned(),
                session: SESSION,
            })
            .await?;
        match harness.expect("rejection").await? {
            Packet::Reject { .. } => Ok(()),
            other => bail!("expected Reject, got {other:?}"),
        }
    });

    step!("enter remote is acknowledged", async {
        harness
            .send(&Packet::EnterRemote {
                session: SESSION,
                edge: ScreenEdge::Right,
                ratio: 0.5,
                width: 1920,
                height: 1080,
            })
            .await?;
        harness.expect_ack("ack after enter").await
    });

    step!("remote state is acknowledged", async {
        harness
            .send(&Packet::SetState {
                session: SESSION,
                remote: true,
            })
            .await?;
        harness.expect_ack("ack after set-state").await
    });

    step!("mouse move back to the edge returns control to the host", async {
        harness
            .send(&Packet::Input {
                session: SESSION,
                sequence: 1,
                event: InputEvent::MouseMove { dx: -120, dy: 0 },
            })
            .await?;
        harness
            .send(&Packet::Input {
                session: SESSION,
                sequence: 2,
                event: InputEvent::MouseMove { dx: 240, dy: 0 },
            })
            .await?;
        match harness.expect("edge return").await? {
            Packet::EdgeReturn {
                session,
                edge,
                ratio,
            } if session == SESSION && edge == ScreenEdge::Right => {
                if (ratio - 0.5).abs() > 0.05 {
                    bail!("edge return ratio {ratio} is not near 0.5");
                }
                Ok(())
            }
            other => bail!("expected EdgeReturn(Right), got {other:?}"),
        }
    });

    step!("leaving remote state is acknowledged", async {
        harness
            .send(&Packet::SetState {
                session: SESSION,
                remote: false,
            })
            .await?;
        harness.expect_ack("ack after leaving remote").await
    });

    step!("release-all is accepted", async {
        harness
            .send(&Packet::ReleaseAll { session: SESSION })
            .await?;
        Ok::<(), anyhow::Error>(())
    });

    if failures == 0 {
        println!("all transport checks passed");
        Ok(())
    } else {
        bail!("{failures} check(s) failed")
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let udp_target: SocketAddr = args
        .get(1)
        .map(|value| value.parse().expect("invalid udp address"))
        .unwrap_or_else(|| "127.0.0.1:24901".parse().unwrap());
    let tcp_target: SocketAddr = args
        .get(2)
        .map(|value| value.parse().expect("invalid tcp address"))
        .unwrap_or_else(|| "127.0.0.1:24900".parse().unwrap());
    let token = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| "selftest-token".to_owned());

    let socket = match UdpSocket::bind("127.0.0.1:0").await {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("cannot bind probe socket: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = socket.connect(udp_target).await {
        eprintln!("cannot reach {udp_target}: {error}");
        return ExitCode::FAILURE;
    }

    let harness = Harness { socket, token };
    let result = run(&harness).await;

    if let Err(error) = clipboard_check(tcp_target).await {
        println!("[FAIL] clipboard channel: {error:#}");
        return ExitCode::FAILURE;
    }
    println!("[PASS] clipboard handshake and text write");

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}
