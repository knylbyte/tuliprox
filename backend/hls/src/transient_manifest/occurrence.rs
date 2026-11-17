use crate::{resource_identity::HlsMediaResourceIdentity, TransientResourceKind};
use tuliprox_parser::hls::origin_manifest::{parse_attribute_list, ParsedByteRange};
use url::Url;

type PendingByteRange = (u64, Option<u64>);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TransientMediaPosition {
    pub(crate) msn: u64,
    part: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransientOccurrenceBinding {
    Media { position: TransientMediaPosition, resource: [u8; 32], dependencies: [u8; 32] },
    Dependency([u8; 32]),
}

impl TransientOccurrenceBinding {
    pub(crate) fn position(self) -> Option<TransientMediaPosition> {
        match self {
            Self::Media { position, .. } => Some(position),
            Self::Dependency(_) => None,
        }
    }

    pub(crate) fn id_input(self) -> String { format!("transient-occurrence:v1:{self:?}") }
}

/// Fixed-size semantic cursor; append-only rewrites carry no per-segment history.
#[derive(Clone, Debug, Default)]
pub(super) struct TransientOccurrenceCursor {
    msn: u64,
    msn_exhausted: bool,
    part: u64,
    pending_range: Option<PendingByteRange>,
    previous_segment_range: Option<([u8; 32], ParsedByteRange)>,
    previous_part_range: Option<([u8; 32], ParsedByteRange)>,
    keys: [Option<([u8; 32], [u8; 32])>; 8],
    map: Option<[u8; 32]>,
    pub(super) invalid: bool,
}

impl TransientOccurrenceCursor {
    pub(super) fn observe_line(&mut self, line: &str) {
        if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            match value.trim().parse() {
                Ok(msn) => self.msn = msn,
                Err(_) => self.invalid = true,
            }
        } else if let Some(value) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            self.pending_range = parse_range(value);
            self.invalid |= self.pending_range.is_none();
        }
    }

    pub(super) fn binding(
        &mut self,
        base: &str,
        uri: &str,
        line: &str,
        kind: TransientResourceKind,
        account: u64,
    ) -> Option<TransientOccurrenceBinding> {
        let resolved = Url::parse(base).and_then(|base| base.join(uri));
        let Ok(resolved) = resolved else {
            self.invalid = true;
            return None;
        };
        let logical = HlsMediaResourceIdentity::from_url(resolved.as_str(), None).semantic_key().bytes();
        match kind {
            TransientResourceKind::Key | TransientResourceKind::Map => {
                self.dependency_binding(logical, line, kind, account)
            }
            TransientResourceKind::Segment | TransientResourceKind::Part => {
                if self.msn_exhausted {
                    self.invalid = true;
                    return None;
                }
                let is_part = kind == TransientResourceKind::Part;
                let preload = line.starts_with("#EXT-X-PRELOAD-HINT:");
                let (range, opaque_range) = if is_part { part_range(line) } else { (self.pending_range.take(), None) };
                let previous = if is_part { &mut self.previous_part_range } else { &mut self.previous_segment_range };
                let effective_range = range.and_then(|(length, offset)| {
                    let offset = offset.or_else(|| {
                        previous
                            .filter(|(resource, _)| *resource == logical)
                            .and_then(|(_, range)| range.offset.checked_add(range.length))
                    });
                    offset.map(|offset| ParsedByteRange { length, offset })
                });
                if !preload {
                    *previous = effective_range.map(|range| (logical, range));
                }
                let position = TransientMediaPosition { msn: self.msn, part: is_part.then_some(self.part) };
                let mut resource =
                    HlsMediaResourceIdentity::from_url(resolved.as_str(), effective_range).semantic_key().bytes();
                // The passthrough parser also accepts unsupported implicit ranges without a predecessor.
                // Keep their opaque evidence distinct from both full objects and proven effective ranges.
                if let Some((length, _)) = range.filter(|_| effective_range.is_none()) {
                    let mut hash = blake3::Hasher::new();
                    hash.update(b"unresolved-transient-range\0");
                    hash.update(&resource);
                    hash.update(&length.to_be_bytes());
                    resource = *hash.finalize().as_bytes();
                }
                if let Some(opaque) = opaque_range {
                    let mut hash = blake3::Hasher::new();
                    hash.update(&resource);
                    hash.update(&opaque);
                    resource = *hash.finalize().as_bytes();
                }
                let binding =
                    TransientOccurrenceBinding::Media { position, resource, dependencies: self.dependency_digest() };
                if is_part {
                    if !preload {
                        self.part = self.part.checked_add(1).unwrap_or_else(|| {
                            self.invalid = true;
                            self.part
                        });
                    }
                } else {
                    if let Some(next) = self.msn.checked_add(1) {
                        self.msn = next;
                    } else {
                        self.msn_exhausted = true;
                    }
                    self.part = 0;
                }
                Some(binding)
            }
            TransientResourceKind::Other => None,
        }
    }

    pub(super) fn segment_range(&self) -> Option<ParsedByteRange> {
        self.previous_segment_range.map(|(_, range)| range)
    }

    pub(super) fn current_part_range(&self) -> Option<ParsedByteRange> {
        self.previous_part_range.map(|(_, range)| range)
    }

    fn dependency_binding(
        &mut self,
        logical: [u8; 32],
        line: &str,
        kind: TransientResourceKind,
        account: u64,
    ) -> Option<TransientOccurrenceBinding> {
        let attributes = parse_attribute_list(line.split_once(':')?.1)?;
        let mut attributes: Vec<_> = attributes.into_iter().filter(|(name, _)| name != "URI").collect();
        attributes.sort();
        let mut hash = blake3::Hasher::new();
        hash.update(&logical);
        hash.update(&account.to_be_bytes());
        for (name, value) in &attributes {
            hash.update(&(name.len() as u64).to_be_bytes());
            hash.update(name.as_bytes());
            hash.update(&(value.len() as u64).to_be_bytes());
            hash.update(value.as_bytes());
        }
        if kind == TransientResourceKind::Map {
            hash.update(&self.key_digest());
        }
        let digest = *hash.finalize().as_bytes();
        if kind == TransientResourceKind::Map {
            self.map = Some(digest);
        } else {
            let format =
                attributes.iter().find(|(name, _)| name == "KEYFORMAT").map_or("identity", |(_, value)| value.as_str());
            let format = *blake3::hash(format.as_bytes()).as_bytes();
            if let Some(slot) = self.keys.iter_mut().find(|slot| slot.is_some_and(|(key, _)| key == format)) {
                *slot = Some((format, digest));
            } else if let Some(slot) = self.keys.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some((format, digest));
            } else {
                self.invalid = true;
            }
        }
        Some(TransientOccurrenceBinding::Dependency(digest))
    }

    pub(super) fn disable_identity_key(&mut self) {
        let identity = *blake3::hash(b"identity").as_bytes();
        for slot in &mut self.keys {
            if slot.is_some_and(|(format, _)| format == identity) {
                *slot = None;
            }
        }
    }

    fn key_digest(&self) -> [u8; 32] {
        let mut keys = self.keys;
        keys.sort_unstable();
        let mut hash = blake3::Hasher::new();
        for (_, digest) in keys.into_iter().flatten() {
            hash.update(&digest);
        }
        *hash.finalize().as_bytes()
    }

    fn dependency_digest(&self) -> [u8; 32] {
        let mut hash = blake3::Hasher::new();
        hash.update(&self.key_digest());
        if let Some(map) = self.map {
            hash.update(&map);
        }
        *hash.finalize().as_bytes()
    }
}

fn parse_range(value: &str) -> Option<PendingByteRange> {
    let (length, offset) =
        value.trim().split_once('@').map_or((value.trim(), None), |(length, offset)| (length, Some(offset)));
    let length = length.parse::<u64>().ok().filter(|length| *length > 0)?;
    let offset = offset.map(str::parse::<u64>).transpose().ok()?;
    if offset.is_some_and(|offset| offset.checked_add(length).is_none()) {
        return None;
    }
    Some((length, offset))
}

fn part_range(line: &str) -> (Option<PendingByteRange>, Option<[u8; 32]>) {
    let Some(attributes) = line.split_once(':').and_then(|(_, value)| parse_attribute_list(value)) else {
        return (None, None);
    };
    let range = attributes.get("BYTERANGE").and_then(|value| parse_range(value)).or_else(|| {
        let start = attributes.get("BYTERANGE-START")?.parse().ok()?;
        let length = attributes.get("BYTERANGE-LENGTH")?.parse().ok()?;
        Some((length, Some(start)))
    });
    let mut fields: Vec<_> = attributes.into_iter().filter(|(name, _)| name.starts_with("BYTERANGE")).collect();
    fields.sort();
    let opaque = (range.is_none() && !fields.is_empty())
        .then(|| *blake3::hash(format!("unresolved-part-range:{fields:?}").as_bytes()).as_bytes());
    (range, opaque)
}
