use super::*;
use crate::transient_manifest::TransientRewriteOptions;

const BASE: &str = "https://first.example/live/index.m3u8?account=transport";

fn rewrite(body: &str, base: &str, account: u64, now: u64) -> TransientRewriteResult {
    TransientManifestRewriter::rewrite_with_options(
        body,
        base,
        &ProxySessionId("occurrence".into()),
        b"secret",
        now,
        100,
        TransientRewriteOptions { origin_account_generation: account, ..TransientRewriteOptions::default() },
    )
}

fn commit(
    state: &mut TransientPassthroughState,
    candidate: TransientRewriteResult,
    now: u64,
) -> crate::transient::HlsTransientManifestFootprint {
    assert!(candidate.checkpoint.has_valid_occurrences());
    state
        .commit_rewritten_manifest_with_semantics(
            candidate.body.clone(),
            candidate.resources,
            now,
            Some(12_000),
            parse_manifest_semantics(&candidate.body),
        )
        .unwrap()
}

fn body(msn: u64, suffix: &str) -> String {
    format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{msn}\n{suffix}")
}

#[test]
fn overlapping_media_keeps_its_route_fetch_target_and_cache_revision_across_host_and_account_changes() {
    let first = body(10, "#EXTINF:4,\nshared.ts\n#EXTINF:4,\nshared.ts\n");
    let mut state = TransientPassthroughState::default();
    commit(&mut state, rewrite(&first, BASE, 0, 1), 1);
    let old_body = state.last_manifest_body.clone().unwrap();
    let old = state
        .resources
        .values()
        .find(|resource| resource.occurrence_binding.as_deref().unwrap().position().unwrap().msn == 11)
        .unwrap()
        .clone();
    let proxy = ProxySessionId("occurrence".into());
    let super::super::TransientObjectFetchDecision::Fetch(token) = state.begin_object_fetch(&proxy, &old, "ts", 2, 100)
    else {
        panic!("new fill");
    };
    assert!(state.mark_object_ready_if_current(&token, "video/mp2t".into(), 10, 3, 100));
    for account in [0, 1] {
        if account == 1 {
            state.invalidate_origin_resources();
        }
        let next = body(11, "#EXTINF:4,\nshared.ts\n#EXTINF:4,\nnext.ts\n");
        commit(&mut state, rewrite(&next, &BASE.replace("first.example", "second.example"), account, 4), 4);
        assert!(old_body.contains(&old.id.0));
        assert!(state.last_manifest_body.as_ref().unwrap().contains(&old.id.0));
        let retained = &state.resources[&old.id];
        assert_eq!(retained.resolved_origin_uri, old.resolved_origin_uri);
        assert_eq!(retained.revision, old.revision);
        assert_eq!(retained.origin_account_generation, 0);
        let key = TransientPassthroughState::transient_object_key(&proxy, &old.id, "ts");
        assert!(state.ready_object(&key, TransientResourceKind::Segment, 5).is_some());
        assert_eq!(state.origin_resource_is_fetchable(&old, 5), account == 0);
    }
    let next = body(13, "#EXTINF:4,\nnext.ts\n");
    commit(&mut state, rewrite(&next, BASE, 1, 6), 6);
    assert!(state.resources.values().any(|resource| resource.origin_account_generation == 1));
}

#[test]
fn overlapping_query_path_and_effective_range_changes_leave_the_published_snapshot_untouched() {
    for changed in ["shared.ts?token=new", "other/shared.ts", "shared.ts"] {
        let mut state = TransientPassthroughState::default();
        let first = body(10, "#EXT-X-BYTERANGE:5@0\n#EXTINF:4,\nshared.ts\n");
        commit(&mut state, rewrite(&first, BASE, 0, 1), 1);
        let old_body = state.last_manifest_body.clone();
        let old_resources = state.resources.clone();
        let old_generation = state.manifest_generation();
        let range = if changed == "shared.ts" { "5@5" } else { "5@0" };
        let candidate = rewrite(&body(10, &format!("#EXT-X-BYTERANGE:{range}\n#EXTINF:4,\n{changed}\n")), BASE, 0, 2);
        assert!(matches!(
            state.commit_rewritten_manifest_with_semantics(
                candidate.body.clone(),
                candidate.resources,
                2,
                Some(4_000),
                parse_manifest_semantics(&candidate.body)
            ),
            Err(HlsTransientManifestCommitError::OccurrenceConflict)
        ));
        assert_eq!(state.last_manifest_body, old_body);
        assert_eq!(state.resources, old_resources);
        assert_eq!(state.manifest_generation(), old_generation);
    }
}

#[test]
fn repeated_uri_has_distinct_segment_and_part_positions_with_equivalent_range_spellings() {
    let suffix = "#EXT-X-PART:DURATION=1,URI=\"shared.ts\",BYTERANGE=\"5@0\"\n#EXT-X-PART:DURATION=1,URI=\"shared.ts\",BYTERANGE=\"5@5\"\n#EXT-X-BYTERANGE:10@0\n#EXTINF:4,\nshared.ts\n#EXT-X-PART:DURATION=1,URI=\"shared.ts\",BYTERANGE=\"5@0\"\n#EXT-X-BYTERANGE:10\n#EXTINF:4,\nshared.ts\n";
    let first = rewrite(&body(10, suffix), BASE, 0, 1);
    assert_eq!(first.resources.len(), 5);
    assert_eq!(first.resources.iter().map(|resource| &resource.id).collect::<HashSet<_>>().len(), 5);
    assert_eq!(first.resources.iter().map(|resource| &resource.resolved_origin_uri).collect::<HashSet<_>>().len(), 1);
    let explicit = rewrite(&body(10, &suffix.replace("#EXT-X-BYTERANGE:10\n", "#EXT-X-BYTERANGE:10@10\n")), BASE, 0, 1);
    assert_eq!(
        first.resources.iter().map(|resource| &resource.id).collect::<HashSet<_>>(),
        explicit.resources.iter().map(|resource| &resource.id).collect::<HashSet<_>>()
    );
    let next_playlist = rewrite(&body(10, suffix), &BASE.replace("index.m3u8", "other.m3u8"), 0, 1);
    assert_eq!(
        first.resources.iter().map(|resource| &resource.id).collect::<HashSet<_>>(),
        next_playlist.resources.iter().map(|resource| &resource.id).collect::<HashSet<_>>()
    );
}

#[test]
fn encrypted_dependency_handoff_rejects_overlap_and_qualifies_new_key_map_and_ciphertext() {
    let suffix = "#EXT-X-KEY:METHOD=AES-128,URI=\"shared.key\"\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\nshared.ts\n";
    let mut state = TransientPassthroughState::default();
    commit(&mut state, rewrite(&body(10, suffix), BASE, 0, 1), 1);
    let old_body = state.last_manifest_body.clone();
    let old = state.resources.clone();
    let proxy = ProxySessionId("occurrence".into());
    let media = old.values().find(|resource| resource.kind == TransientResourceKind::Segment).unwrap();
    let super::super::TransientObjectFetchDecision::Fetch(token) =
        state.begin_object_fetch(&proxy, media, "ts", 2, 100)
    else {
        panic!("new fill");
    };
    state.invalidate_origin_resources();
    let overlap = rewrite(&body(10, suffix), BASE, 1, 3);
    assert!(matches!(
        state.commit_rewritten_manifest_with_semantics(
            overlap.body.clone(),
            overlap.resources,
            3,
            Some(4_000),
            parse_manifest_semantics(&overlap.body)
        ),
        Err(HlsTransientManifestCommitError::OccurrenceConflict)
    ));
    assert_eq!(state.last_manifest_body, old_body);
    assert!(!state.mark_object_ready_if_current(&token, "video/mp2t".into(), 10, 4, 100));
    commit(&mut state, rewrite(&body(11, suffix), BASE, 1, 5), 5);
    for kind in [TransientResourceKind::Key, TransientResourceKind::Map, TransientResourceKind::Segment] {
        let previous = old.values().find(|resource| resource.kind == kind).unwrap();
        let current = state
            .resources
            .values()
            .find(|resource| resource.kind == kind && resource.origin_account_generation == 1)
            .unwrap();
        assert_ne!(previous.id, current.id);
        assert_eq!(previous.resolved_origin_uri, current.resolved_origin_uri);
        assert_eq!(&state.resources[&previous.id], previous);
        assert!(!state.origin_resource_is_fetchable(previous, 6));
        assert!(state.origin_resource_is_fetchable(current, 6));
    }
}

#[test]
fn append_checkpoint_preserves_msn_parts_ranges_and_dependencies() {
    let prefix = body(10, "#EXT-X-PLAYLIST-TYPE:EVENT\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"shared.key\"\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXT-X-BYTERANGE:10@0\n#EXTINF:4,\nshared.ts\n");
    let suffix =
        "#EXT-X-PART:DURATION=1,URI=\"part.m4s\"\n#EXT-X-BYTERANGE:10\n#EXTINF:4,\nshared.ts\n#EXT-X-ENDLIST\n";
    let first = rewrite(&prefix, BASE, 0, 1);
    let append = TransientManifestRewriter::rewrite_append(
        suffix,
        BASE,
        &ProxySessionId("occurrence".into()),
        b"secret",
        2,
        100,
        first.checkpoint,
    );
    let full = rewrite(&format!("{prefix}{suffix}"), BASE, 0, 2);
    assert!(append.checkpoint.has_valid_occurrences());
    assert_eq!(format!("{}{suffix}", first.body).matches("#EXT-X-ENDLIST").count(), 1);
    assert_eq!(format!("{}{}", first.body, append.body), full.body);
    let rotated = rewrite(&format!("{prefix}{suffix}"), "https://second.example/live/reload.m3u8?delivery=2", 0, 2);
    assert_eq!(full.body, rotated.body);
    let mut state = TransientPassthroughState::default();
    let footprint = commit(&mut state, rotated, 2);
    assert!(footprint.estimated_metadata_bytes >= state.resources.len() * std::mem::size_of::<TransientResourceRef>());
}

#[test]
fn preload_hint_tracks_the_next_part_without_merging_other_msns_or_unknown_ranges() {
    let first = rewrite(
        &body(10, "#EXT-X-PART:DURATION=1,URI=\"shared.m4s\"\n#EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"next.m4s\"\n"),
        BASE,
        0,
        1,
    );
    let completed = rewrite(&body(10, "#EXT-X-PART:DURATION=1,URI=\"shared.m4s\"\n#EXT-X-PART:DURATION=1,URI=\"next.m4s\"\n#EXTINF:4,\nsegment.ts\n"), BASE, 0, 2);
    for resource in &first.resources {
        assert!(completed.resources.iter().any(|next| next.id == resource.id));
    }
    let next_msn = rewrite(&body(11, "#EXT-X-PART:DURATION=1,URI=\"shared.m4s\"\n"), BASE, 0, 2);
    assert!(first.resources.iter().all(|resource| resource.id != next_msn.resources[0].id));
    let hints: Vec<_> = [0, 5]
        .into_iter()
        .map(|start| {
            rewrite(
                &body(10, &format!("#EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"shared.m4s\",BYTERANGE-START={start}\n")),
                BASE,
                0,
                1,
            )
            .resources[0]
                .id
                .clone()
        })
        .collect();
    assert_ne!(hints[0], hints[1]);
}

#[test]
fn manifest_rotation_keeps_routes_until_relative_resolution_changes_the_resource() {
    let original = body(10, "#EXTINF:4,\nshared.ts\n");
    let mut state = TransientPassthroughState::default();
    commit(&mut state, rewrite(&original, BASE, 0, 1), 1);
    let old_body = state.last_manifest_body.clone();
    for url in [BASE.replace("index.m3u8", "other.m3u8"), format!("{BASE}&delivery=other")] {
        commit(&mut state, rewrite(&original, &url, 0, 2), 2);
        assert_eq!(state.last_manifest_body, old_body);
    }
    let different_base = BASE.replace("/live/", "/different/");
    let candidate = rewrite(&original, &different_base, 0, 3);
    assert!(matches!(
        state.commit_rewritten_manifest_with_semantics(
            candidate.body.clone(),
            candidate.resources,
            3,
            Some(4_000),
            parse_manifest_semantics(&candidate.body),
        ),
        Err(HlsTransientManifestCommitError::OccurrenceConflict)
    ));
    assert_eq!(state.last_manifest_body, old_body);
}

#[test]
fn published_ranges_remain_resolvable_after_full_and_event_append_rewrites() {
    use tuliprox_parser::hls::origin_manifest::{
        parse_origin_media_manifest, OriginManifestParseOutcome, ParsedByteRange,
    };
    let prefix =
        body(10, "#EXT-X-VERSION:4\n#EXT-X-PLAYLIST-TYPE:EVENT\n#EXT-X-BYTERANGE:5@0\n#EXTINF:4,\nshared.ts\n");
    let suffix = "#EXT-X-BYTERANGE:5\n#EXTINF:4,\nshared.ts\n";
    let first = rewrite(&prefix, BASE, 0, 1);
    let appended = TransientManifestRewriter::rewrite_append(
        suffix,
        BASE,
        &ProxySessionId("occurrence".into()),
        b"secret",
        2,
        100,
        first.checkpoint,
    );
    let full = rewrite(&format!("{prefix}{suffix}"), BASE, 0, 2);
    assert_eq!(format!("{}{}", first.body, appended.body), full.body);
    assert!(full.body.contains("#EXT-X-BYTERANGE:5@5\n"));
    // The origin parser does not implement EVENT; retain every URI/range line for this range-contract check.
    let range_body = full.body.replace("#EXT-X-PLAYLIST-TYPE:EVENT\n", "");
    let OriginManifestParseOutcome::Normal(parsed) = parse_origin_media_manifest(&range_body, BASE) else {
        panic!(
            "rewritten ranges must resolve: {:?}",
            tuliprox_parser::hls::origin_manifest::parse_origin_media_manifest(&range_body, BASE)
        );
    };
    assert_ne!(parsed.segments[0].resolved_origin_url, parsed.segments[1].resolved_origin_url);
    assert_eq!(parsed.segments[1].origin_byte_range, Some(ParsedByteRange { length: 5, offset: 5 }));
}

#[test]
fn maximum_msn_is_publishable_until_a_successor_is_actually_assigned() {
    for (start, suffix) in
        [(u64::MAX, "#EXTINF:4,\nlast.ts\n"), (u64::MAX - 1, "#EXTINF:4,\nfirst.ts\n#EXTINF:4,\nlast.ts\n")]
    {
        let mut state = TransientPassthroughState::default();
        let source = body(start, suffix);
        state
            .commit_origin_manifest_for_test(&source, BASE, &ProxySessionId("occurrence".into()), b"secret", 1)
            .unwrap();
        let candidate = rewrite(&source, BASE, 0, 1);
        assert!(candidate.checkpoint.has_valid_occurrences());
        let endlist = TransientManifestRewriter::rewrite_append(
            "#EXT-X-ENDLIST\n",
            BASE,
            &ProxySessionId("occurrence".into()),
            b"secret",
            2,
            100,
            candidate.checkpoint.clone(),
        );
        assert!(endlist.checkpoint.has_valid_occurrences());
        for next in ["#EXTINF:4,\nwrapped.ts\n", "#EXT-X-PART:DURATION=1,URI=\"wrapped.m4s\"\n"] {
            let overflow = TransientManifestRewriter::rewrite_append(
                next,
                BASE,
                &ProxySessionId("occurrence".into()),
                b"secret",
                2,
                100,
                candidate.checkpoint.clone(),
            );
            assert!(!overflow.checkpoint.has_valid_occurrences(), "a real successor cannot wrap");
        }
        let old = state.last_manifest_body.clone();
        assert!(state
            .commit_origin_manifest_for_test(
                &format!("{source}#EXTINF:4,\nwrapped.ts\n"),
                BASE,
                &ProxySessionId("occurrence".into()),
                b"secret",
                2
            )
            .is_err());
        assert_eq!(state.last_manifest_body, old);
    }
}

#[test]
fn retained_publication_conflicts_survive_ttl_and_resource_cleanup_without_granting_fetch_access() {
    let original = body(10, "#EXTINF:4,\nshared.ts?token=old\n");
    for now in [50, 101, 102] {
        for cleanup in [false, true] {
            let mut state = TransientPassthroughState::new(100);
            state
                .commit_origin_manifest_for_test(&original, BASE, &ProxySessionId("occurrence".into()), b"secret", 1)
                .unwrap();
            let old_body = state.last_manifest_body.clone();
            let old_generation = state.manifest_generation();
            let resource = state.resources.values().next().unwrap().clone();
            if cleanup {
                state.prune_expired(now);
            }
            assert!(
                state.resources.contains_key(&resource.id),
                "the retained current publication keeps bounded semantic evidence"
            );
            assert_eq!(state.origin_resource_is_fetchable(&resource, now), now <= 101);
            for changed in ["shared.ts?token=new", "other.ts?token=old"] {
                assert!(state
                    .commit_origin_manifest_for_test(
                        &body(10, &format!("#EXTINF:4,\n{changed}\n")),
                        BASE,
                        &ProxySessionId("occurrence".into()),
                        b"secret",
                        now
                    )
                    .is_err());
                assert_eq!(state.last_manifest_body, old_body);
                assert_eq!(state.manifest_generation(), old_generation);
            }
            assert!(state
                .commit_origin_manifest_for_test(
                    &body(10, "#EXT-X-BYTERANGE:5@0\n#EXTINF:4,\nshared.ts?token=old\n"),
                    BASE,
                    &ProxySessionId("occurrence".into()),
                    b"secret",
                    now,
                )
                .is_err());
            assert_eq!(state.last_manifest_body, old_body);
            state
                .commit_origin_manifest_for_test(
                    &body(11, "#EXTINF:4,\nnew.ts\n"),
                    BASE,
                    &ProxySessionId("occurrence".into()),
                    b"secret",
                    now,
                )
                .unwrap();
            state.prune_expired(1_000);
            assert!(!state.resources.contains_key(&resource.id));
            assert_eq!(state.resources.len(), 1, "only the bounded current publication survives");
        }
    }
}

#[test]
fn part_byte_range_attributes_are_explicit_when_occurrence_routes_differ() {
    let candidate = rewrite(&body(10, "#EXT-X-PART:DURATION=1,URI=\"shared.m4s\",BYTERANGE=\"5@0\"\n#EXT-X-PART:DURATION=1,URI=\"shared.m4s\",BYTERANGE=\"5\"\n#EXTINF:4,\nsegment.ts\n"), BASE, 0, 1);
    assert!(candidate.body.contains("BYTERANGE=\"5@5\""));
    assert_eq!(candidate.resources.iter().filter(|resource| resource.kind == TransientResourceKind::Part).count(), 2);
}
