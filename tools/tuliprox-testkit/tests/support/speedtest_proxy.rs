//! Local, run-owned HTTP relay that paces bytes written to the client TCP socket.
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

const RATE_BPS: u32 = 6_000_000;
const CHUNK_BYTES: usize = 8192;

pub async fn start(target: SocketAddr) -> std::io::Result<(SocketAddr, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                if let Err(error) = relay(stream, target).await {
                    eprintln!("speedtest relay error: {error}");
                }
            });
        }
    });
    Ok((address, task))
}

async fn relay(mut client: TcpStream, target: SocketAddr) -> std::io::Result<()> {
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        if request.len() >= 16 * 1024 {
            return Err(std::io::Error::other("proxy request headers exceed 16 KiB"));
        }
        let byte = client.read_u8().await?;
        request.push(byte);
    }
    let request = std::str::from_utf8(&request).map_err(std::io::Error::other)?;
    let mut lines = request.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default();
    let url = url::Url::parse(first.next().unwrap_or_default()).map_err(std::io::Error::other)?;
    if method != "GET"
        || url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port_or_known_default() != Some(target.port())
    {
        return Err(std::io::Error::other("proxy only accepts GETs to its local fixture"));
    }
    let query = url.query().map_or_else(String::new, |value| format!("?{value}"));
    let mut outgoing = format!("GET {}{query} HTTP/1.1\r\n", url.path());
    for line in lines.filter(|line| !line.is_empty() && !line.to_ascii_lowercase().starts_with("connection:")) {
        outgoing.push_str(line);
        outgoing.push_str("\r\n");
    }
    outgoing.push_str("Connection: close\r\n\r\n");
    let mut upstream = TcpStream::connect(target).await?;
    upstream.write_all(outgoing.as_bytes()).await?;
    let mut buffer = [0_u8; CHUNK_BYTES];
    let mut deadline = tokio::time::Instant::now();
    loop {
        let n = upstream.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        let bytes = u32::try_from(n).map_err(std::io::Error::other)?;
        let duration = Duration::from_secs_f64(f64::from(bytes) * 8.0 / f64::from(RATE_BPS));
        let now = tokio::time::Instant::now();
        if now > deadline + duration {
            deadline = now;
        }
        deadline += duration;
        tokio::time::sleep_until(deadline).await;
        client.write_all(&buffer[..n]).await?;
    }
    client.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn relay_transfers_complete_response_at_bounded_network_rate() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let target = listener.local_addr()?;
        let upstream = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 131072\r\nConnection: close\r\n\r\n").await.unwrap();
            socket.write_all(&vec![42_u8; 131_072]).await.unwrap();
        });
        let (address, proxy) = start(target).await?;
        let client = reqwest::Client::builder().proxy(reqwest::Proxy::all(format!("http://{address}"))?).build()?;
        let started = tokio::time::Instant::now();
        let response = client.get(format!("http://{target}/segment")).send().await?.bytes().await?;
        let elapsed = started.elapsed();
        proxy.abort();
        upstream.await?;
        assert_eq!(response.len(), 131_072);
        assert!(response.iter().all(|byte| *byte == 42));
        assert!(elapsed >= Duration::from_millis(174));
        Ok(())
    }
}
