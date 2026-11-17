//! Synthetic HLS timing and representation checks. These measure bytes, not decoded pictures.
use crate::{
    frame::{Frame, FrameDecoder, FrameValidator},
    protocol::{OriginStreamId, RunId},
    TestkitError,
};
use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::{fmt, num::NonZeroU32, time::Duration};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupMode {
    Conservative,
    FirstReady,
    Progressive,
    Optimistic,
}

impl fmt::Display for StartupMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Conservative => "conservative",
            Self::FirstReady => "first_ready",
            Self::Progressive => "progressive",
            Self::Optimistic => "optimistic",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HlsOriginProfile {
    pub window_segments: u32,
    /// Target and advertised segment duration in seconds.
    pub target_duration_secs: u32,
    pub prefix_delay_millis: u64,
    pub missing_head_segments: u32,
    pub abort_segments: Vec<u64>,
    pub unknown_length: bool,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "segment_duration")]
    pub segment_duration_secs: Option<u32>,
    /// Advance the live window with wall-clock time instead of the manifest request count, so a
    /// client polling faster than one segment sees the window move at the advertised cadence.
    #[serde(default, skip_serializing_if = "is_false")]
    pub realtime: bool,
    /// Number of valid frames sent before the delayed tail.
    pub prefix_frames: u32,
    /// Optional size of each prefix chunk; zero sends the entire prefix at once.
    pub prefix_chunk_bytes: usize,
    /// Delay between prefix chunks, excluding the first chunk.
    pub prefix_chunk_interval_millis: u64,
    /// Delay after response headers and before the first body bytes, indexed by segment sequence.
    pub initial_body_delays_millis: Vec<u64>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde calls this with a reference to the field
fn is_false(value: &bool) -> bool { !*value }

impl Default for HlsOriginProfile {
    fn default() -> Self {
        Self {
            window_segments: 6,
            target_duration_secs: 4,
            prefix_delay_millis: 1000,
            missing_head_segments: 0,
            abort_segments: Vec::new(),
            unknown_length: false,
            segment_duration_secs: None,
            realtime: false,
            prefix_frames: 1,
            prefix_chunk_bytes: 0,
            prefix_chunk_interval_millis: 0,
            initial_body_delays_millis: Vec::new(),
        }
    }
}

impl HlsOriginProfile {
    pub fn validate(&self) -> Result<(), TestkitError> {
        if !(3..=64).contains(&self.window_segments)
            || !(1..=60).contains(&self.target_duration_secs)
            || self.missing_head_segments >= self.window_segments
            || self.prefix_delay_millis > 10_000
            || self.abort_segments.len() > 64
            || !(1..=8192).contains(&self.prefix_frames)
            || self.prefix_chunk_bytes > 1024 * 1024
            || self.prefix_chunk_interval_millis > 10_000
            || self.initial_body_delays_millis.len() > 64
            || self.initial_body_delays_millis.iter().any(|delay| *delay > 10_000)
        {
            return Err(TestkitError::Configuration(
                "HLS profile requires 3..64 segments, target duration 1..60 seconds, a shorter missing head, at most 64 aborts, delays <= 10000 ms, 1..8192 prefix frames and chunk size <= 1 MiB"
                    .to_owned(),
            ));
        }
        if let Some(duration) = self.segment_duration_secs {
            if !(1..=60).contains(&duration) {
                return Err(TestkitError::Configuration(
                    "HLS profile segment duration must be between 1 and 60 seconds".to_owned(),
                ));
            }
        }
        if self.realtime && self.segment_duration_secs.is_none() {
            return Err(TestkitError::Configuration(
                "HLS profile realtime mode requires segment_duration_secs".to_owned(),
            ));
        }
        Ok(())
    }
}

pub fn segment_parts(
    run: &RunId,
    marker: u32,
    segment: u64,
    prefix_frames: u32,
) -> Result<(Bytes, Bytes), TestkitError> {
    let first = segment
        .checked_mul(u64::from(prefix_frames) + 1)
        .ok_or_else(|| TestkitError::Protocol("HLS frame sequence overflow".to_owned()))?;
    let second = first
        .checked_add(u64::from(prefix_frames))
        .ok_or_else(|| TestkitError::Protocol("HLS frame sequence overflow".to_owned()))?;
    let stream = OriginStreamId::new(format!("hls-presentation-{marker}"));
    let mut prefix = BytesMut::new();
    let mut tail = BytesMut::new();
    for sequence in first..second {
        Frame::synthetic(run, &stream, marker, sequence).encode(&mut prefix)?;
    }
    Frame::synthetic(run, &stream, marker, second).encode(&mut tail)?;
    Ok((prefix.freeze(), tail.freeze()))
}

pub fn segment_stream(
    parts: (Bytes, Bytes),
    profile: &HlsOriginProfile,
    sequence: u64,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static {
    let delay = Duration::from_millis(profile.prefix_delay_millis);
    let abort = profile.abort_segments.contains(&sequence);
    let chunk_size = if profile.prefix_chunk_bytes == 0 { parts.0.len().max(1) } else { profile.prefix_chunk_bytes };
    let chunk_delay = Duration::from_millis(profile.prefix_chunk_interval_millis);
    let initial_delay = Duration::from_millis(
        usize::try_from(sequence)
            .ok()
            .and_then(|index| profile.initial_body_delays_millis.get(index))
            .copied()
            .unwrap_or(0),
    );
    let prefix = futures::stream::unfold((parts.0, 0, None), move |(prefix, offset, next)| async move {
        if offset >= prefix.len() {
            return None;
        }
        if offset == 0 && !initial_delay.is_zero() {
            tokio::time::sleep(initial_delay).await;
        } else if offset > 0 {
            if let Some(deadline) = next {
                tokio::time::sleep_until(deadline).await;
            }
        }
        let end = offset.saturating_add(chunk_size).min(prefix.len());
        let now = tokio::time::Instant::now();
        let next = next.filter(|deadline| now <= *deadline + chunk_delay).unwrap_or(now) + chunk_delay;
        Some((Ok(prefix.slice(offset..end)), (prefix, end, Some(next))))
    });
    prefix.chain(futures::stream::once(async move {
        tokio::time::sleep(delay).await;
        if abort {
            Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "injected HLS origin body drop"))
        } else {
            Ok(parts.1)
        }
    }))
}

#[derive(Debug, Default)]
pub struct ProbeOptions {
    pub verify_revision: bool,
    pub expect_body_error: bool,
    pub max_first_byte_millis: Option<u64>,
    pub min_prefix_to_eof_millis: Option<u64>,
    /// Optional simulated application delivery rate for the measured segment body, in bits per second.
    pub client_delivery_rate_bps: Option<NonZeroU32>,
    /// Optional HTTP proxy used for all probe requests.
    pub http_proxy: Option<String>,
}

/// Paces delivery to the measuring application; this does not throttle the underlying socket.
struct ClientDeliveryLimiter {
    rate: NonZeroU32,
    deadline: Option<tokio::time::Instant>,
}

impl ClientDeliveryLimiter {
    async fn deliver(&mut self, count: u32) {
        let duration = Duration::from_secs_f64(f64::from(count) * 8.0 / f64::from(self.rate.get()));
        let now = tokio::time::Instant::now();
        let deadline = self.deadline.map_or(now, |previous| previous.max(now)) + duration;
        self.deadline = Some(deadline);
        tokio::time::sleep_until(deadline).await;
    }
}

#[derive(Debug, Serialize)]
pub struct ProbeResult {
    pub playlist_millis: f64,
    pub segment_headers_millis: f64,
    pub entry_to_first_byte_millis: f64,
    pub entry_to_4096_millis: Option<f64>,
    pub first_byte_to_4096_millis: Option<f64>,
    pub entry_to_body_end_millis: f64,
    pub prefix_to_body_end_millis: f64,
    pub received_bytes: usize,
    pub valid_frames: u64,
    pub body_error: bool,
    pub retry_identical: bool,
    pub range_identical: bool,
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY: usize = 1024 * 1024;

/// All requests share one absolute deadline. Errors after a prefix must be observable as errors.
pub async fn probe(url: &str, run: &RunId, marker: u32, options: &ProbeOptions) -> Result<ProbeResult, TestkitError> {
    if options.verify_revision && options.expect_body_error {
        return Err(TestkitError::Configuration("revision verification requires a complete body".to_owned()));
    }
    tokio::time::timeout(PROBE_TIMEOUT, probe_inner(url, run, marker, options))
        .await
        .map_err(|_| TestkitError::Protocol("HLS probe exceeded absolute deadline".to_owned()))?
}

async fn first_segment(client: &reqwest::Client, url: &str) -> Result<url::Url, TestkitError> {
    let mut url = url::Url::parse(url).map_err(|error| TestkitError::Configuration(error.to_string()))?;
    for _ in 0..8 {
        let response = client.get(url).send().await?.error_for_status()?;
        url = response.url().clone();
        let manifest = response.text().await?;
        let uri = manifest
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .ok_or_else(|| TestkitError::Protocol("HLS probe found no segment URI".to_owned()))?;
        url = url.join(uri).map_err(|error| TestkitError::Configuration(error.to_string()))?;
        if !manifest.lines().any(|line| line.starts_with("#EXT-X-STREAM-INF")) {
            return Ok(url);
        }
    }
    Err(TestkitError::Protocol("HLS probe master nesting limit".to_owned()))
}

async fn collect(response: reqwest::Response) -> Result<Bytes, TestkitError> {
    let mut body = response.bytes_stream();
    let mut data = BytesMut::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        if chunk.len() > MAX_BODY.saturating_sub(data.len()) {
            return Err(TestkitError::Protocol("HLS probe body exceeds 1 MiB limit".to_owned()));
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data.freeze())
}

async fn verify_revision(client: &reqwest::Client, url: &url::Url, bytes: &[u8]) -> Result<(), TestkitError> {
    let retry = collect(client.get(url.clone()).send().await?.error_for_status()?).await?;
    if retry != bytes {
        return Err(TestkitError::Protocol("same HLS URI returned different retry bytes".to_owned()));
    }
    let end = bytes.len().saturating_sub(1).min(63);
    let response = client
        .get(url.clone())
        .header(reqwest::header::RANGE, format!("bytes=0-{end}"))
        .send()
        .await?
        .error_for_status()?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(TestkitError::Protocol(
            "bound HLS revision did not return HTTP 206 for a completed range".to_owned(),
        ));
    }
    let expected = format!("bytes 0-{end}/{}", bytes.len());
    if response.headers().get(reqwest::header::CONTENT_RANGE).and_then(|value| value.to_str().ok())
        != Some(expected.as_str())
    {
        return Err(TestkitError::Protocol("bound HLS revision returned a different Content-Range".to_owned()));
    }
    if collect(response).await?.as_ref() != &bytes[..=end] {
        return Err(TestkitError::Protocol("HLS range offsets do not match the original revision".to_owned()));
    }
    Ok(())
}

async fn probe_inner(url: &str, run: &RunId, marker: u32, options: &ProbeOptions) -> Result<ProbeResult, TestkitError> {
    let started = tokio::time::Instant::now();
    let mut builder = reqwest::Client::builder().http1_only();
    if let Some(proxy) = &options.http_proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy)?).pool_max_idle_per_host(0);
    }
    let client = builder.build()?;
    let segment_url = first_segment(&client, url).await?;
    let segment_started = tokio::time::Instant::now();
    let response = client.get(segment_url.clone()).send().await?.error_for_status()?;
    let headers_at = tokio::time::Instant::now();
    let expected_length = response.content_length();
    let mut stream = response.bytes_stream();
    let mut first_byte = None;
    let mut first_4096 = None;
    let mut body_error = false;
    let mut bytes = BytesMut::new();
    let mut limiter = options.client_delivery_rate_bps.map(|rate| ClientDeliveryLimiter { rate, deadline: None });
    while let Some(chunk) = stream.next().await {
        if let Ok(chunk) = chunk {
            if chunk.len() > MAX_BODY.saturating_sub(bytes.len()) {
                return Err(TestkitError::Protocol("HLS probe body exceeds 1 MiB limit".to_owned()));
            }
            let mut offset = 0;
            while offset < chunk.len() {
                let until_marker = if bytes.is_empty() {
                    1
                } else if bytes.len() < 4096 {
                    4096 - bytes.len()
                } else {
                    usize::MAX
                };
                let size = (chunk.len() - offset).min(until_marker);
                if let Some(limiter) = &mut limiter {
                    let count =
                        u32::try_from(size).map_err(|_| TestkitError::Protocol("client chunk too large".to_owned()))?;
                    limiter.deliver(count).await;
                }
                bytes.extend_from_slice(&chunk[offset..offset + size]);
                offset += size;
                let delivered_at = tokio::time::Instant::now();
                first_byte.get_or_insert(delivered_at);
                if first_4096.is_none() && bytes.len() >= 4096 {
                    first_4096 = Some(delivered_at);
                }
            }
        } else {
            body_error = true;
            break;
        }
    }
    let ended = tokio::time::Instant::now();
    let first_byte = first_byte.ok_or_else(|| TestkitError::Protocol("HLS probe received no prefix".to_owned()))?;
    if body_error != options.expect_body_error {
        return Err(TestkitError::Protocol(format!(
            "HLS body error: expected {}, observed {body_error}",
            options.expect_body_error
        )));
    }
    if !body_error && expected_length.is_some_and(|length| length != bytes.len() as u64) {
        return Err(TestkitError::Protocol("HLS response ended before Content-Length".to_owned()));
    }
    let mut validator = FrameValidator::new(run, marker);
    let mut frames = 0;
    for frame in FrameDecoder::default().push(&bytes)? {
        validator.validate(&frame)?;
        frames += 1;
    }
    if frames == 0 {
        return Err(TestkitError::Protocol("HLS probe found no valid synthetic frame".to_owned()));
    }
    let millis = |duration: Duration| duration.as_secs_f64() * 1000.0;
    let mut result = ProbeResult {
        playlist_millis: millis(segment_started - started),
        segment_headers_millis: millis(headers_at - segment_started),
        entry_to_first_byte_millis: millis(first_byte - started),
        entry_to_4096_millis: first_4096.map(|instant| millis(instant - started)),
        first_byte_to_4096_millis: first_4096.map(|instant| millis(instant - first_byte)),
        entry_to_body_end_millis: millis(ended - started),
        prefix_to_body_end_millis: millis(ended - first_byte),
        received_bytes: bytes.len(),
        valid_frames: frames,
        body_error,
        retry_identical: false,
        range_identical: false,
    };
    if options.max_first_byte_millis.is_some_and(|max| first_byte - started > Duration::from_millis(max))
        || options.min_prefix_to_eof_millis.is_some_and(|min| ended - first_byte < Duration::from_millis(min))
    {
        return Err(TestkitError::Protocol("HLS probe timing assertion failed".to_owned()));
    }
    if options.verify_revision {
        verify_revision(&client, &segment_url, &bytes).await?;
        result.retry_identical = true;
        result.range_identical = true;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::get,
        Router,
    };

    #[tokio::test(start_paused = true)]
    async fn simulated_client_delivery_limits_4096_bytes_to_80_kilobits_per_second() {
        let mut limiter = ClientDeliveryLimiter { rate: NonZeroU32::new(80_000).unwrap(), deadline: None };
        let started = tokio::time::Instant::now();
        limiter.deliver(1).await;
        let first_byte = tokio::time::Instant::now();
        limiter.deliver(4095).await;
        assert!(started.elapsed() >= Duration::from_micros(409_600));
        assert!(first_byte.elapsed() >= Duration::from_micros(409_500));
        tokio::time::sleep(Duration::from_secs(1)).await;
        let after_idle = tokio::time::Instant::now();
        limiter.deliver(1000).await;
        assert!(after_idle.elapsed() >= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn probe_measures_paced_first_byte_and_4096_byte_delivery() -> Result<(), TestkitError> {
        let run = RunId::new("paced-probe");
        let (prefix, tail) = segment_parts(&run, 17, 0, 64)?;
        let body = Bytes::from([prefix.as_ref(), tail.as_ref()].concat());
        let app = Router::new().route("/index.m3u8", get(|| async { "#EXTM3U\n#EXTINF:4,\n0.ts\n" })).route(
            "/0.ts",
            get(move || {
                let body = body.clone();
                async move { body }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/index.m3u8", listener.local_addr()?);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let result = probe(
            &url,
            &run,
            17,
            &ProbeOptions { client_delivery_rate_bps: NonZeroU32::new(80_000), ..ProbeOptions::default() },
        )
        .await;
        server.abort();
        let result = result?;
        assert!(result.first_byte_to_4096_millis.is_some_and(|elapsed| elapsed >= 400.0));
        assert!(result.entry_to_body_end_millis >= result.entry_to_4096_millis.unwrap());
        assert!(result.received_bytes >= 4096);
        Ok(())
    }

    #[test]
    fn large_prefix_contains_valid_frames_and_exceeds_4096_bytes() -> Result<(), TestkitError> {
        let run = RunId::new("large-prefix");
        let (prefix, tail) = segment_parts(&run, 17, 0, 64)?;
        assert!(prefix.len() >= 4096);
        let mut decoder = FrameDecoder::default();
        let mut validator = FrameValidator::new(&run, 17);
        let frames = decoder.push(&[prefix.as_ref(), tail.as_ref()].concat())?;
        assert_eq!(frames.len(), 65);
        for frame in frames {
            validator.validate(&frame)?;
        }
        Ok(())
    }

    #[test]
    fn speedtest_segments_are_near_750_kilobytes() -> Result<(), TestkitError> {
        let run = RunId::new("speedtest-size");
        for sequence in [0, 3, 5] {
            let (prefix, tail) = segment_parts(&run, 17, sequence, 7200)?;
            assert!((700_000..=800_000).contains(&(prefix.len() + tail.len())));
        }
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn chunked_prefix_is_paced_before_delayed_tail() -> Result<(), TestkitError> {
        let profile = HlsOriginProfile {
            prefix_chunk_bytes: 2,
            prefix_chunk_interval_millis: 100,
            prefix_delay_millis: 1000,
            ..HlsOriginProfile::default()
        };
        let stream = segment_stream((Bytes::from_static(b"abcde"), Bytes::from_static(b"z")), &profile, 0);
        futures::pin_mut!(stream);
        let started = tokio::time::Instant::now();
        assert_eq!(stream.next().await.unwrap()?, Bytes::from_static(b"ab"));
        assert_eq!(stream.next().await.unwrap()?, Bytes::from_static(b"cd"));
        assert_eq!(started.elapsed(), Duration::from_millis(100));
        assert_eq!(stream.next().await.unwrap()?, Bytes::from_static(b"e"));
        assert_eq!(started.elapsed(), Duration::from_millis(200));
        assert_eq!(stream.next().await.unwrap()?, Bytes::from_static(b"z"));
        assert_eq!(started.elapsed(), Duration::from_millis(1200));
        assert!(stream.next().await.is_none());
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn continuous_origin_paces_chunks_without_tail_pause() -> Result<(), TestkitError> {
        let profile = HlsOriginProfile {
            prefix_chunk_bytes: 1000,
            prefix_chunk_interval_millis: 100,
            prefix_delay_millis: 0,
            ..Default::default()
        };
        let stream = segment_stream((Bytes::from(vec![1_u8; 2000]), Bytes::from_static(b"tail")), &profile, 0);
        futures::pin_mut!(stream);
        let started = tokio::time::Instant::now();
        assert_eq!(stream.next().await.unwrap()?.len(), 1000);
        assert_eq!(started.elapsed(), Duration::ZERO);
        assert_eq!(stream.next().await.unwrap()?.len(), 1000);
        assert_eq!(started.elapsed(), Duration::from_millis(100));
        assert_eq!(stream.next().await.unwrap()?, Bytes::from_static(b"tail"));
        assert!(started.elapsed() < Duration::from_millis(102));
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn each_segment_uses_its_own_initial_body_delay() -> Result<(), TestkitError> {
        let profile = HlsOriginProfile { initial_body_delays_millis: vec![0, 700, 1200], ..Default::default() };
        let started = tokio::time::Instant::now();
        let first = segment_stream((Bytes::from_static(b"a"), Bytes::new()), &profile, 0);
        futures::pin_mut!(first);
        assert_eq!(first.next().await.unwrap()?, Bytes::from_static(b"a"));
        assert_eq!(started.elapsed(), Duration::ZERO);
        let second = segment_stream((Bytes::from_static(b"b"), Bytes::new()), &profile, 1);
        futures::pin_mut!(second);
        assert_eq!(second.next().await.unwrap()?, Bytes::from_static(b"b"));
        assert_eq!(started.elapsed(), Duration::from_millis(700));
        let third = segment_stream((Bytes::from_static(b"c"), Bytes::new()), &profile, 2);
        futures::pin_mut!(third);
        assert_eq!(third.next().await.unwrap()?, Bytes::from_static(b"c"));
        assert_eq!(started.elapsed(), Duration::from_millis(1900));
        Ok(())
    }

    #[tokio::test]
    async fn http_headers_arrive_before_delayed_body() -> Result<(), TestkitError> {
        let profile = HlsOriginProfile { initial_body_delays_millis: vec![0, 600], ..Default::default() };
        let app =
            Router::new().route(
                "/segment",
                get(move || {
                    let profile = profile.clone();
                    async move {
                        Body::from_stream(segment_stream((Bytes::from_static(b"prefix"), Bytes::new()), &profile, 1))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/segment", listener.local_addr()?);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let response = tokio::time::timeout(Duration::from_secs(2), reqwest::get(url))
            .await
            .map_err(|_| TestkitError::Protocol("headers timed out".to_owned()))??;
        let mut body = response.bytes_stream();
        assert!(tokio::time::timeout(Duration::from_millis(200), body.next()).await.is_err());
        let bytes = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .map_err(|_| TestkitError::Protocol("body timed out".to_owned()))?
            .ok_or_else(|| TestkitError::Protocol("body missing".to_owned()))??;
        assert_eq!(bytes, Bytes::from_static(b"prefix"));
        server.abort();
        Ok(())
    }

    async fn server(abort: bool, change_retry: bool) -> Result<(String, tokio::task::JoinHandle<()>), TestkitError> {
        let run = RunId::new("hls-probe-test");
        let parts = segment_parts(&run, 17, 0, 1)?;
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let app = Router::new().route("/index.m3u8", get(|| async { "#EXTM3U\n#EXTINF:4,\n0.ts\n" })).route(
            "/0.ts",
            get(move |headers: HeaderMap| {
                let parts = parts.clone();
                let attempts = std::sync::Arc::clone(&attempts);
                async move {
                    if headers.contains_key("range") {
                        let mut response = (StatusCode::PARTIAL_CONTENT, parts.0.slice(..64)).into_response();
                        response.headers_mut().insert(
                            "content-range",
                            format!("bytes 0-63/{}", parts.0.len() + parts.1.len())
                                .parse()
                                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                        );
                        return Ok::<_, StatusCode>(response);
                    }
                    let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if change_retry && attempt > 0 {
                        return Ok("changed revision".into_response());
                    }
                    let profile = HlsOriginProfile {
                        prefix_delay_millis: 100,
                        abort_segments: if abort { vec![0] } else { Vec::new() },
                        ..HlsOriginProfile::default()
                    };
                    Ok(Body::from_stream(segment_stream(parts, &profile, 0)).into_response())
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/index.m3u8", listener.local_addr()?);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok((url, server))
    }

    #[tokio::test]
    async fn measures_prefix_before_eof_and_checks_retry_and_range() -> Result<(), TestkitError> {
        let (url, server) = server(false, false).await?;
        let result = probe(
            &url,
            &RunId::new("hls-probe-test"),
            17,
            &ProbeOptions { verify_revision: true, min_prefix_to_eof_millis: Some(50), ..ProbeOptions::default() },
        )
        .await;
        server.abort();
        let result = result?;
        assert_eq!(result.valid_frames, 2);
        assert!(result.retry_identical && result.range_identical);
        Ok(())
    }

    #[tokio::test]
    async fn truncated_chunked_body_is_an_error_after_valid_prefix() -> Result<(), TestkitError> {
        let (url, server) = server(true, false).await?;
        let result = probe(
            &url,
            &RunId::new("hls-probe-test"),
            17,
            &ProbeOptions { expect_body_error: true, ..ProbeOptions::default() },
        )
        .await;
        server.abort();
        assert!(result?.body_error);
        Ok(())
    }

    #[tokio::test]
    async fn detects_new_revision_on_same_uri() -> Result<(), TestkitError> {
        let (url, server) = server(false, true).await?;
        let result = probe(
            &url,
            &RunId::new("hls-probe-test"),
            17,
            &ProbeOptions { verify_revision: true, ..ProbeOptions::default() },
        )
        .await;
        server.abort();
        assert!(matches!(result, Err(TestkitError::Protocol(message)) if message.contains("different retry bytes")));
        Ok(())
    }

    #[test]
    fn rejects_unbounded_or_empty_profiles() {
        assert!(HlsOriginProfile { window_segments: 2, ..HlsOriginProfile::default() }.validate().is_err());
        assert!(HlsOriginProfile { missing_head_segments: 6, ..HlsOriginProfile::default() }.validate().is_err());
        assert!(HlsOriginProfile { segment_duration_secs: Some(0), ..HlsOriginProfile::default() }.validate().is_err());
        assert!(HlsOriginProfile { segment_duration_secs: Some(61), ..HlsOriginProfile::default() }
            .validate()
            .is_err());
        assert!(HlsOriginProfile { segment_duration_secs: Some(2), ..HlsOriginProfile::default() }.validate().is_ok());
        assert!(HlsOriginProfile::default().validate().is_ok());
    }

    #[test]
    fn realtime_profile_requires_a_segment_duration() {
        assert!(HlsOriginProfile { realtime: true, ..HlsOriginProfile::default() }.validate().is_err());
        assert!(HlsOriginProfile { realtime: true, segment_duration_secs: Some(2), ..HlsOriginProfile::default() }
            .validate()
            .is_ok());
        assert!(!HlsOriginProfile::default().realtime);
    }

    #[test]
    fn realtime_flag_round_trips_through_json() {
        let parsed: HlsOriginProfile =
            serde_json::from_str(r#"{"segment_duration_secs":2,"realtime":true}"#).expect("profile parses");
        assert!(parsed.realtime);
        let serialized = serde_json::to_string(&HlsOriginProfile::default()).expect("profile serializes");
        assert!(!serialized.contains("realtime"), "the default must not reach the origin command line: {serialized}");
    }
}
