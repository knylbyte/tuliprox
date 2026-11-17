use super::*;

#[tokio::test]
async fn media_timestamp_wrap_keeps_the_continuing_msn_and_origin_epoch() {
    use tuliprox_mpegts::transport_stream_buffer::{HlsFiniteTsRenderSpec, TransportStreamBuffer};
    let origin = Origin::start().await;
    let buffer = TransportStreamBuffer::new(media(1));
    let profile = buffer.finite_hls_timestamp_profile().unwrap();
    let modulus = 1_u64 << 33;
    let offset = (modulus * 2 - 270_000 - profile.first_clock_90khz) % modulus;
    let render = |offset| {
        buffer
            .render_finite_hls_segment(HlsFiniteTsRenderSpec {
                timestamp_offset_ticks_90khz: offset,
                continuity_seed: 0,
                logical_segment_index: 0,
            })
            .unwrap()
            .to_vec()
    };
    let before = render(offset);
    let after = render(offset + 360_000);
    let before_clock = TransportStreamBuffer::new(before.clone()).finite_hls_timestamp_profile().unwrap();
    let after_clock = TransportStreamBuffer::new(after.clone()).finite_hls_timestamp_profile().unwrap();
    assert!(before_clock.first_clock_90khz > after_clock.first_clock_90khz);
    assert_eq!((after_clock.first_clock_90khz + modulus - before_clock.first_clock_90khz) % modulus, 360_000);
    *origin.state.bytes.write().await = before;
    let request = ready_window(&origin).await;
    let original = request.session.read().await.segments[&3].occurrence_identity;
    *origin.state.bytes.write().await = after.clone();
    crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &window(&origin, 104, 2, "old"),
        HlsManifestCommitAcceptanceMode::StrictPinnedHost,
    )
    .await
    .unwrap();
    fetch(&request, 4).await;
    let state = request.session.read().await;
    assert_eq!(state.origin_epoch, 0);
    assert_eq!(state.segments[&3].occurrence_identity, original);
    assert_eq!(state.segments[&4].occurrence_identity.unwrap().origin_msn(), 105);
    assert!(state.segments.values().all(|entry| !entry.discontinuity_before));
    let expected: [u8; 32] = Sha256::digest(after).into();
    assert_eq!(state.origin_content.digest(&state.segments[&4], state.activity.origin_work_generation), Some(expected));
    println!(
        "media clock wrap: before={} after={} forward_ticks=360000 msn=104..105 epoch=0",
        before_clock.first_clock_90khz, after_clock.first_clock_90khz
    );
}

#[tokio::test]
async fn maximum_msn_has_no_wrapping_successor_in_the_same_namespace() {
    let request = test_origin_refresh_request(test_session());
    let maximum = format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{}\n#EXTINF:4,\nmax.ts\n", u64::MAX);
    let fetched = fetched_manifest(&maximum);
    {
        let mut state = request.session.write().await;
        commit_fetched_manifest(&mut state, &fetched, &request, request.now_ms).unwrap();
        assert_eq!(HlsCacheOriginAnchor::capture(&state).unwrap().identity.origin_msn(), u64::MAX);
    }
    assert!(crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &fetched,
        HlsManifestCommitAcceptanceMode::StrictPinnedHost
    )
    .await
    .is_ok());
    let reset = fetched_manifest("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4,\nmax.ts\n");
    assert!(crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &reset,
        HlsManifestCommitAcceptanceMode::StrictPinnedHost
    )
    .await
    .is_err());
    assert_eq!(request.session.read().await.origin_epoch, 0);
    let overflow = format!("{maximum}#EXTINF:4,\noverflow.ts\n");
    let tuliprox_parser::hls::origin_manifest::OriginManifestParseOutcome::Normal(mut parsed) =
        tuliprox_parser::hls::origin_manifest::parse_origin_media_manifest(&maximum, &fetched.final_manifest_url)
    else {
        panic!("expected maximum MSN fixture");
    };
    let mut successor = parsed.segments[0].clone();
    successor.origin_seq = 0;
    parsed.segments.push(successor);
    let state = request.session.read().await;
    assert_eq!(
        HlsCacheOriginAnchor::capture(&state).unwrap().validate(&state, &parsed),
        Err(HlsManifestRejectLogReason::OriginCacheSuccessorMissing {
            previous_origin_seq: u64::MAX,
            candidate_origin_seq: 0
        })
    );
    assert!(matches!(
        tuliprox_parser::hls::origin_manifest::parse_origin_media_manifest(&overflow, &fetched.final_manifest_url),
        tuliprox_parser::hls::origin_manifest::OriginManifestParseOutcome::TransientPassthrough { .. }
    ));
}
