use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use tuliprox_parser::hls::origin_manifest::{ParsedByteRange, ParsedOriginSegment};
use url::Url;

const RESOURCE_IDENTITY_DOMAIN: &[u8] = b"tuliprox/hls/media-resource/v1\0";
const QUERY_IDENTITY_DOMAIN: &[u8] = b"tuliprox/hls/media-resource-query/v1\0";
const MAX_PUBLISHED_RESOURCE_HISTORY: usize = 1_024;

/// Complete serialized path/range and query. This identifies a resource binding, not a segment occurrence.
/// Hashes keep credentials out of diagnostics without normalizing URL components.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HlsMediaResourceIdentity {
    exact_path: [u8; 32],
    query: [u8; 32],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct HlsMediaResourceSemanticKey([u8; 32]);

/// Logical namespace of the selected stream in one input and archive context.
/// Manifest redirects can change the resolution base without changing this namespace.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HlsMediaPlaylistScope([u8; 32]);

impl HlsMediaPlaylistScope {
    pub fn from_session(key: &crate::HlsSessionKey) -> Option<Self> {
        if key.stream_ref.is_empty() {
            return None;
        }
        let mut hasher = Sha256::new();
        hasher.update(b"tuliprox/hls/media-playlist-scope/v1\0");
        hasher.update(key.input_id.to_be_bytes());
        hasher.update((key.stream_ref.len() as u64).to_be_bytes());
        hasher.update(key.stream_ref.as_bytes());
        hasher.update([u8::from(key.archive_reference.is_some())]);
        if let Some(reference) = key.archive_reference {
            hasher.update(reference.to_be_bytes());
        }
        hasher.update([u8::from(key.archive_identity.is_some())]);
        if let Some(identity) = &key.archive_identity {
            hasher.update((identity.len() as u64).to_be_bytes());
            hasher.update(identity.as_bytes());
        }
        Some(Self(hasher.finalize().into()))
    }
}

/// Immutable playlist evidence. Transport hosts, HTTP headers and account/work generations are excluded.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HlsSegmentOccurrenceIdentity {
    scope: HlsMediaPlaylistScope,
    origin_msn: u64,
    resource: HlsMediaResourceIdentity,
}

/// A concrete, content-qualified alternative for one immutable cached occurrence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HlsVerifiedOccurrenceBinding {
    pub(crate) proxy_seq: u64,
    pub(crate) original: HlsSegmentOccurrenceIdentity,
    pub(crate) candidate: HlsSegmentOccurrenceIdentity,
}

impl HlsVerifiedOccurrenceBinding {
    pub(crate) fn matches(
        self,
        proxy_seq: u64,
        original: HlsSegmentOccurrenceIdentity,
        candidate: HlsSegmentOccurrenceIdentity,
    ) -> bool {
        self.proxy_seq == proxy_seq && self.original == original && self.candidate == candidate
    }
}

impl HlsSegmentOccurrenceIdentity {
    pub(crate) const fn origin_msn(self) -> u64 { self.origin_msn }

    pub(crate) const fn playlist_scope(self) -> HlsMediaPlaylistScope { self.scope }

    pub(crate) const fn resource(self) -> HlsMediaResourceIdentity { self.resource }

    pub(crate) fn same_scope(self, other: Self) -> bool { self.scope == other.scope }

    pub fn same_position(self, other: Self) -> bool { self.same_scope(other) && self.origin_msn == other.origin_msn }

    pub fn semantic_key(self) -> HlsMediaResourceSemanticKey {
        let mut hasher = Sha256::new();
        hasher.update(b"tuliprox/hls/segment-occurrence/v1\0");
        hasher.update(self.scope.0);
        hasher.update(self.origin_msn.to_be_bytes());
        hasher.update(self.resource.semantic_key().bytes());
        HlsMediaResourceSemanticKey(hasher.finalize().into())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(marker: u8) -> Self {
        Self {
            scope: HlsMediaPlaylistScope([0; 32]),
            origin_msn: u64::from(marker),
            resource: HlsMediaResourceIdentity::for_test(marker),
        }
    }

    pub fn from_parsed(scope: HlsMediaPlaylistScope, segment: &ParsedOriginSegment) -> Self {
        Self {
            scope,
            origin_msn: segment.origin_seq,
            resource: HlsMediaResourceIdentity::from_url(&segment.resource_uri, segment.origin_byte_range),
        }
    }
}

impl HlsMediaResourceIdentity {
    pub fn from_url(url: &str, byte_range: Option<ParsedByteRange>) -> Self {
        let parsed = Url::parse(url).ok();
        let path = parsed.as_ref().map_or_else(|| url.split(['?', '#']).next().unwrap_or_default(), Url::path);
        let query = parsed.as_ref().map_or_else(
            || url.split('#').next().and_then(|url| url.split_once('?').map(|(_, query)| query)),
            Url::query,
        );
        let mut hasher = Sha256::new();
        hasher.update(QUERY_IDENTITY_DOMAIN);
        hasher.update([u8::from(query.is_some())]);
        if let Some(query) = query {
            hasher.update(query.as_bytes());
        }
        Self {
            exact_path: hash_identity(RESOURCE_IDENTITY_DOMAIN, path.as_bytes(), byte_range),
            query: hasher.finalize().into(),
        }
    }

    pub fn matches(self, other: Self) -> bool { self == other }

    pub(crate) fn same_path_and_range(self, other: Self) -> bool { self.exact_path == other.exact_path }

    pub const fn exact_path_hash(self) -> [u8; 32] { self.exact_path }

    pub fn semantic_key(self) -> HlsMediaResourceSemanticKey {
        let mut hasher = Sha256::new();
        hasher.update(self.exact_path);
        hasher.update(self.query);
        HlsMediaResourceSemanticKey(hasher.finalize().into())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub const fn for_test(marker: u8) -> Self { Self { exact_path: [marker; 32], query: [0; 32] } }
}

impl HlsMediaResourceSemanticKey {
    pub const fn bytes(self) -> [u8; 32] { self.0 }

    pub fn diagnostic_token(self) -> [u8; 8] {
        let mut token = [0_u8; 8];
        token.copy_from_slice(&self.0[..8]);
        token
    }

    #[cfg(any(test, feature = "test-support"))]
    pub const fn for_test(bytes: [u8; 32]) -> Self { Self(bytes) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PublishedResource {
    identity: HlsSegmentOccurrenceIdentity,
    proxy_seq: u64,
}

/// Bounded history of segment occurrences actually exposed by this HLS session.
///
/// It deliberately outlives individual `SegmentEntry` values so cache/timeline GC cannot
/// make previously published origin media look new again. Dropping the session drops the
/// history and its stream-local namespace.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HlsPublishedResourceHistory {
    resources: VecDeque<PublishedResource>,
    generation: u64,
    restart_proxy_floor: u64,
}

impl HlsPublishedResourceHistory {
    pub fn is_empty(&self) -> bool { self.resources.is_empty() }

    pub const fn generation(&self) -> u64 { self.generation }

    pub(crate) const fn restart_proxy_floor(&self) -> u64 { self.restart_proxy_floor }

    /// Start a new occurrence namespace after an accepted origin restart.
    /// Historical entries remain retained for old leases and diagnostics.
    pub(crate) fn begin_restart(&mut self, next_proxy_seq: u64) { self.restart_proxy_floor = next_proxy_seq; }

    pub(crate) fn entries_across_epochs(&self) -> impl Iterator<Item = (HlsSegmentOccurrenceIdentity, u64)> + '_ {
        self.resources.iter().map(|published| (published.identity, published.proxy_seq))
    }

    pub fn binding_for(&self, identity: HlsSegmentOccurrenceIdentity) -> Option<(HlsSegmentOccurrenceIdentity, u64)> {
        self.resources
            .iter()
            .rev()
            .find(|published| {
                published.proxy_seq >= self.restart_proxy_floor && published.identity.same_position(identity)
            })
            .map(|published| (published.identity, published.proxy_seq))
    }

    pub fn proxy_seq_for(&self, identity: HlsSegmentOccurrenceIdentity) -> Option<u64> {
        self.resources
            .iter()
            .rev()
            .find(|published| published.proxy_seq >= self.restart_proxy_floor && published.identity == identity)
            .map(|published| published.proxy_seq)
    }

    pub fn record(&mut self, identity: HlsSegmentOccurrenceIdentity, proxy_seq: u64) {
        if proxy_seq < self.restart_proxy_floor {
            return;
        }
        if let Some(published) = self
            .resources
            .iter_mut()
            .rev()
            .find(|published| published.proxy_seq >= self.restart_proxy_floor && published.identity == identity)
        {
            published.proxy_seq = proxy_seq;
            return;
        }
        if self.resources.len() == MAX_PUBLISHED_RESOURCE_HISTORY {
            self.resources.pop_front();
        }
        self.resources.push_back(PublishedResource { identity, proxy_seq });
        self.generation = self.generation.saturating_add(1);
    }

    pub fn recent_entries(&self, limit: usize) -> impl Iterator<Item = (HlsSegmentOccurrenceIdentity, u64)> + '_ {
        self.resources
            .iter()
            .rev()
            .filter(|published| published.proxy_seq >= self.restart_proxy_floor)
            .take(limit)
            .map(|published| (published.identity, published.proxy_seq))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn len(&self) -> usize { self.resources.len() }
}

fn hash_identity(domain: &[u8], path: &[u8], byte_range: Option<ParsedByteRange>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(path);
    hasher.update([0]);
    match byte_range {
        Some(byte_range) => {
            hasher.update([1]);
            hasher.update(byte_range.offset.to_be_bytes());
            hasher.update(byte_range.length.to_be_bytes());
        }
        None => hasher.update([0]),
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::{
        HlsMediaResourceIdentity, HlsMediaResourceSemanticKey, HlsPublishedResourceHistory,
        MAX_PUBLISHED_RESOURCE_HISTORY,
    };
    use tuliprox_parser::hls::origin_manifest::ParsedByteRange;

    #[test]
    fn different_hex_parent_paths_are_distinct_resources() {
        let first = HlsMediaResourceIdentity::from_url("https://origin.example/live/0123456789abcdef/0.ts", None);
        let second = HlsMediaResourceIdentity::from_url("https://origin.example/live/fedcba9876543210/0.ts", None);
        assert!(!first.matches(second));
        assert_ne!(first.semantic_key(), second.semantic_key());
    }

    #[test]
    fn full_path_and_byte_range_survive_identity_comparison() {
        let base = url::Url::parse("https://origin.example/live/index.m3u8").unwrap();
        let paths = [
            "../one/0.ts",
            "../two/0.ts",
            "../0123456789abcdef/0.ts",
            "../a-long-non-hexadecimal-directory/0.ts",
            "../one%2Ftwo/0.ts",
            "../one/two/0.ts",
        ];
        let ranges = [
            None,
            Some(ParsedByteRange { length: 188, offset: 0 }),
            Some(ParsedByteRange { length: 188, offset: 188 }),
            Some(ParsedByteRange { length: 376, offset: 0 }),
        ];
        for (path_index, path) in paths.iter().enumerate() {
            for (other_index, other) in paths.iter().enumerate() {
                for (range_index, range) in ranges.iter().enumerate() {
                    for (other_range_index, other_range) in ranges.iter().enumerate() {
                        let left = HlsMediaResourceIdentity::from_url(base.join(path).unwrap().as_str(), *range);
                        let right =
                            HlsMediaResourceIdentity::from_url(base.join(other).unwrap().as_str(), *other_range);
                        assert_eq!(left.matches(right), path_index == other_index && range_index == other_range_index);
                    }
                }
            }
        }
    }

    #[test]
    fn query_identity_preserves_unknown_repeated_and_empty_parameters() {
        let queries = ["", "?", "?token=a", "?token=b", "?x=", "?x", "?x=1&x=2", "?x=2&x=1", "?x=%2F", "?x=/"];
        for (index, left) in queries.iter().enumerate() {
            for (other_index, right) in queries.iter().enumerate() {
                let left = HlsMediaResourceIdentity::from_url(&format!("https://origin.example/live/0.ts{left}"), None);
                let right =
                    HlsMediaResourceIdentity::from_url(&format!("https://origin.example/live/0.ts{right}"), None);
                assert_eq!(left.matches(right), index == other_index, "queries {index} and {other_index}");
            }
        }
    }

    #[test]
    fn identity_preserves_query_path_and_range_but_ignores_fragment() {
        let first = HlsMediaResourceIdentity::from_url("https://a.example/live/segment.ts?token=one#x", None);
        let second = HlsMediaResourceIdentity::from_url("https://b.example/live/segment.ts?token=one#y", None);
        let other_path = HlsMediaResourceIdentity::from_url("https://b.example/other/segment.ts", None);
        let ranged = HlsMediaResourceIdentity::from_url(
            "https://b.example/live/segment.ts",
            Some(ParsedByteRange { length: 10, offset: 20 }),
        );

        assert!(first.matches(second));
        assert!(!first.matches(other_path));
        assert!(!first.matches(ranged));
    }

    #[test]
    fn full_parent_path_remains_part_of_the_semantic_key() {
        let first = HlsMediaResourceIdentity::from_url(
            "https://a.example/channel-one/0123456789abcdef/1745180_474.ts?token=one",
            None,
        );
        let rotated = HlsMediaResourceIdentity::from_url(
            "https://b.example/channel-one/fedcba9876543210/1745180_474.ts?token=two",
            None,
        );
        let other_stream =
            HlsMediaResourceIdentity::from_url("https://b.example/channel-two/fedcba9876543210/1745180_474.ts", None);

        assert!(!first.matches(rotated));
        assert!(!first.matches(other_stream));
        assert_ne!(first, rotated, "ordinary identity equality remains exact");
        assert_ne!(first.semantic_key(), rotated.semantic_key());
        assert_ne!(first.semantic_key(), other_stream.semantic_key());
    }

    #[test]
    fn root_level_volatile_parent_does_not_collapse_identity_to_basename() {
        let first = HlsMediaResourceIdentity::from_url("https://a.example/0123456789abcdef/segment.ts", None);
        let second = HlsMediaResourceIdentity::from_url("https://b.example/fedcba9876543210/segment.ts", None);

        assert!(!first.matches(second));
        assert_ne!(first.semantic_key(), second.semantic_key());
    }

    #[test]
    fn semantic_log_token_is_never_identity_evidence() {
        let mut first_bytes = [0x11; 32];
        let mut second_bytes = first_bytes;
        first_bytes[31] = 0x22;
        second_bytes[31] = 0x33;
        let first = HlsMediaResourceSemanticKey::for_test(first_bytes);
        let second = HlsMediaResourceSemanticKey::for_test(second_bytes);

        assert_eq!(first.diagnostic_token(), second.diagnostic_token());
        assert_ne!(first, second);
        assert_eq!(first.diagnostic_token(), first.bytes()[..8]);
    }

    #[test]
    fn published_history_is_bounded_and_preserves_query_identity() {
        let mut history = HlsPublishedResourceHistory::default();
        for sequence in 0..=MAX_PUBLISHED_RESOURCE_HISTORY {
            history.record(
                occurrence(
                    &format!("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:{sequence}\n#EXTINF:4,\n{sequence}.ts\n"),
                    "https://origin.example/live/index.m3u8",
                    &crate::HlsSessionKey::new(1, "history"),
                ),
                u64::try_from(sequence).unwrap_or(u64::MAX),
            );
        }
        assert_eq!(history.len(), MAX_PUBLISHED_RESOURCE_HISTORY);
        assert_eq!(history.generation(), 1_025);
        assert_eq!(
            history.proxy_seq_for(occurrence(
                "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1024\n#EXTINF:4,\n1024.ts\n",
                "https://other.example/live/index.m3u8",
                &crate::HlsSessionKey::new(1, "history")
            )),
            Some(1_024)
        );
        history.record(
            occurrence(
                "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1024\n#EXTINF:4,\n1024.ts\n",
                "https://other.example/live/index.m3u8",
                &crate::HlsSessionKey::new(1, "history"),
            ),
            1_024,
        );
        assert_eq!(history.generation(), 1_025, "re-recording an identity must not advance evidence");
    }

    #[test]
    fn published_history_updates_the_same_newest_match_that_lookup_returns() {
        let oldest = super::HlsSegmentOccurrenceIdentity::for_test(1);
        let newest = super::HlsSegmentOccurrenceIdentity::for_test(2);
        let next = newest;
        let mut history = HlsPublishedResourceHistory {
            resources: std::collections::VecDeque::from([
                super::PublishedResource { identity: oldest, proxy_seq: 10 },
                super::PublishedResource { identity: newest, proxy_seq: 20 },
            ]),
            generation: 2,
            restart_proxy_floor: 0,
        };

        history.record(next, 30);

        assert_eq!(history.proxy_seq_for(next), Some(30));
        assert_eq!(history.resources.front().map(|resource| resource.proxy_seq), Some(10));
        assert_eq!(history.resources.back().map(|resource| resource.proxy_seq), Some(30));
        assert_eq!(history.generation(), 2);
    }
    fn occurrence(body: &str, base: &str, key: &crate::HlsSessionKey) -> super::HlsSegmentOccurrenceIdentity {
        let tuliprox_parser::hls::origin_manifest::OriginManifestParseOutcome::Normal(manifest) =
            tuliprox_parser::hls::origin_manifest::parse_origin_media_manifest(body, base)
        else {
            panic!("normal manifest");
        };
        super::HlsSegmentOccurrenceIdentity::from_parsed(
            super::HlsMediaPlaylistScope::from_session(key).expect("known playlist"),
            manifest.segments.last().unwrap(),
        )
    }

    #[test]
    fn occurrence_identity_equates_uri_spellings_and_transport_hosts() {
        let key = crate::HlsSessionKey::new(7, "stream-one");
        let expected = occurrence(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:101\n#EXTINF:4,\nseg101.ts?token=x\n",
            "https://manifest-a.example/live/index.m3u8",
            &key,
        );
        for base in [
            "https://manifest-a.example/live/index.m3u8",
            "http://manifest-b.example:8080/live/rotation.m3u8?delivery=2",
        ] {
            for reference in [
                "seg101.ts?token=x",
                "./seg101.ts?token=x",
                "/live/seg101.ts?token=x",
                "//segment-b.example:9000/live/seg101.ts?token=x",
                "https://cdn-a.example/live/seg101.ts?token=x",
                "http://cdn-b.example:8081/live/seg101.ts?token=x",
            ] {
                let body = format!("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:101\n#EXTINF:4,\n{reference}\n");
                assert_eq!(expected, occurrence(&body, base, &key), "reference={reference}, base={base}");
            }
        }
    }

    #[test]
    fn occurrence_identity_separates_msn_and_logical_sources() {
        let base = "https://origin.example/live/index.m3u8";
        let body = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:101\n#EXTINF:4,\n/common.ts\n";
        let key = crate::HlsSessionKey::new(7, "stream-one");
        let expected = occurrence(body, base, &key);
        for other in [
            crate::HlsSessionKey::new(8, "stream-one"),
            crate::HlsSessionKey::new(7, "stream-two"),
            key.clone().with_archive_reference(1),
            key.clone().with_archive_reference(1).with_archive_identity("other"),
        ] {
            assert_ne!(expected, occurrence(body, base, &other));
        }
        assert_ne!(expected, occurrence(&body.replace("101", "102"), base, &key));
        assert!(super::HlsMediaPlaylistScope::from_session(&crate::HlsSessionKey::new(7, "")).is_none());
    }

    #[test]
    fn manifest_address_changes_only_matter_when_the_resolved_media_changes() {
        let base = "https://origin.example/live/index.m3u8?delivery=1";
        let key = crate::HlsSessionKey::new(7, "stream");
        let body = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:101\n#EXTINF:4,\n/live/media.ts?binding=1\n";
        let original = occurrence(body, base, &key);
        assert_eq!(original, occurrence(body, "https://origin.example/other/reload.m3u8?delivery=1", &key));
        assert_eq!(original, occurrence(body, "https://origin.example/live/index.m3u8?delivery=2", &key));

        let relative = body.replace("/live/media.ts", "media.ts");
        assert_eq!(original, occurrence(&relative, base, &key));
        assert_ne!(original, occurrence(&relative, "https://origin.example/other/index.m3u8?delivery=1", &key));
    }

    #[test]
    fn occurrence_identity_detects_each_path_query_and_hls_range_change() {
        let base = "https://origin.example/live/index.m3u8";
        let key = crate::HlsSessionKey::new(7, "stream");
        let variants = [
            ("/live/seg.ts", ""),
            ("/other/seg.ts", ""),
            ("/live/seg.ts?", ""),
            ("/live/seg.ts?token=alt", ""),
            ("/live/seg.ts?token=neu", ""),
            ("/live/seg.ts?a=1&b=2", ""),
            ("/live/seg.ts?b=2&a=1", ""),
            ("/live/seg.ts", "#EXT-X-BYTERANGE:500@0\n"),
            ("/live/seg.ts", "#EXT-X-BYTERANGE:500@500\n"),
            ("/live/seg.ts", "#EXT-X-BYTERANGE:501@0\n"),
        ];
        let identities: Vec<_> = variants
            .iter()
            .map(|(uri, range)| {
                occurrence(
                    &format!("#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-MEDIA-SEQUENCE:101\n{range}#EXTINF:4,\n{uri}\n"),
                    base,
                    &key,
                )
            })
            .collect();
        for (index, identity) in identities.iter().enumerate() {
            for (other_index, other) in identities.iter().enumerate() {
                assert_eq!(identity == other, index == other_index, "variants {index}, {other_index}");
            }
        }
        let explicit = occurrence("#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-MEDIA-SEQUENCE:101\n#EXT-X-BYTERANGE:500@500\n#EXTINF:4,\n/live/seg.ts\n", base, &key);
        let implicit = occurrence("#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-MEDIA-SEQUENCE:100\n#EXT-X-BYTERANGE:500@0\n#EXTINF:4,\n/live/seg.ts\n#EXT-X-BYTERANGE:500\n#EXTINF:4,\n/live/seg.ts\n", base, &key);
        assert_eq!(explicit, implicit);
    }
}
