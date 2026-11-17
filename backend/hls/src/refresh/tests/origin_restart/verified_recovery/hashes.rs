use super::*;

#[tokio::test]
async fn equal_media_cannot_override_a_changed_dependency_or_prepared_account() {
    for case in ["map", "key", "account"] {
        let origin = Origin::start().await;
        let mut request = ready_window(&origin).await;
        let mut fetched = rotate_tail(window(&origin, 101, 4, "old"), "equal");
        let before = request.session.read().await.segments.clone();
        match case {
            "map" => fetched.body = fetched.body.replace("#EXT-X-TARGETDURATION:4", "#EXT-X-TARGETDURATION:4\n#EXT-X-MAP:URI=\"new-init.mp4\""),
            "key" => fetched.body = fetched.body.replace("#EXT-X-TARGETDURATION:4", "#EXT-X-TARGETDURATION:4\n#EXT-X-KEY:METHOD=AES-128,URI=\"new-key.bin\",IV=0x00000000000000000000000000000001"),
            _ => {
                let mut io = crate::refresh::tests::account_handoff::request_with_origin_work(&request.session).origin_io.unwrap();
                let mut state = request.session.write().await;
                let owner = state.proxy_session_id.0.clone();
                state.replace_origin_account_binding(Some(crate::HlsOriginAccountBinding::rebound(Arc::from("fixture"), Arc::from("old"), owner.clone(), 1, request.now_ms)));
                io.prepared_account = Some(crate::origin_account_runtime::HlsOriginAccountSnapshot::from_session(&state));
                state.replace_origin_account_binding(Some(crate::HlsOriginAccountBinding::rebound(Arc::from("fixture"), Arc::from("new"), owner, 2, request.now_ms)));
                request.origin_io = Some(io);
            }
        }
        let requests = origin.state.requests.lock().await.len();
        assert!(candidate(&request, fetched).await.is_err(), "{case}: byte equality cannot override another gate");
        let state = request.session.read().await;
        assert_eq!(state.segments, before);
        assert_eq!(HlsOriginContentEvidence::verified_bindings(&state), []);
        assert_eq!(origin.state.requests.lock().await.len(), requests, "{case}: rejection precedes candidate I/O");
    }
}

#[tokio::test]
async fn complete_http_hashes_distinguish_equal_payloads_and_a_last_byte_change() {
    let origin = Origin::start().await;
    let request = ready_window(&origin).await;
    let original = request.session.read().await.segments[&3].clone();
    let payload = media(1);
    // Independently calculated with Python hashlib over the actual fixture and null packet.
    let expected = [
        141, 32, 176, 94, 70, 97, 229, 58, 174, 215, 22, 50, 79, 139, 124, 2, 199, 53, 23, 5, 14, 247, 239, 208, 124,
        50, 36, 49, 107, 108, 230, 53,
    ];
    assert_eq!(<[u8; 32]>::from(Sha256::digest(&payload)), expected);
    assert_eq!(request.session.read().await.origin_content.digest(&original, 0), Some(expected));
    let equal = rotate_tail(window(&origin, 101, 4, "old"), "equal");
    assert!(candidate(&request, equal).await.is_ok());
    let mut changed = payload.clone();
    *changed.last_mut().unwrap() ^= 1;
    assert_eq!(changed.len(), payload.len());
    assert_eq!(changed[..changed.len() - 1], payload[..payload.len() - 1]);
    let different: [u8; 32] = Sha256::digest(&changed).into();
    assert_ne!(expected, different);
    *origin.state.bytes.write().await = changed;
    let before = request.session.read().await.segments.clone();
    assert!(candidate(&request, rotate_tail(window(&origin, 101, 4, "old"), "different")).await.is_err());
    assert_eq!(request.session.read().await.segments, before);
    assert_eq!(tokio::fs::read(request.segment_cache.object_path(&original.cache_key)).await.unwrap(), payload);
    let transfers = origin.state.transfers.lock().await;
    assert_eq!(transfers.len(), 6, "four reference reads and two concrete candidate reads");
    assert!(transfers.iter().all(|(_, range, length)| range.is_none() && *length == media(1).len()));
    println!("complete hashes: equal={expected:02x?} last_byte_changed={different:02x?} bytes={}", media(1).len());
}

#[tokio::test]
async fn unfinished_http_and_missing_or_prefix_evidence_cannot_qualify_a_binding() {
    for case in ["truncated", "missing", "prefix"] {
        let origin = Origin::start().await;
        let request = ready_window(&origin).await;
        let original = request.session.read().await.segments.clone();
        if case == "truncated" {
            origin.state.truncate.store(true, Ordering::Release);
        } else {
            let mut state = request.session.write().await;
            state.origin_content = HlsOriginContentEvidence::default();
            if case == "prefix" {
                let entry = state.segments[&3].clone();
                let prefix: [u8; 32] = Sha256::digest(&media(1)[..188]).into();
                let generation = state.activity.origin_work_generation;
                state.origin_content.record(&entry, generation, prefix);
            }
        }
        let result = candidate(&request, rotate_tail(window(&origin, 101, 4, "old"), case)).await;
        assert!(result.is_err(), "{case}: no complete equal raw proof");
        let state = request.session.read().await;
        assert_eq!(state.segments, original);
        assert_eq!(HlsOriginContentEvidence::verified_bindings(&state), []);
    }
    let payload = media(1);
    let digest = OriginContentDigest::default();
    let mut reader = digest.observe(Box::pin(std::io::Cursor::new(payload.clone())));
    reader.read_exact(&mut [0_u8; 188]).await.unwrap();
    assert!(digest.completed().is_none(), "a prefix read cannot publish a digest");
    reader.read_to_end(&mut Vec::new()).await.unwrap();
    let expected: [u8; 32] = Sha256::digest(payload).into();
    assert_eq!(digest.completed(), Some(expected));
}

#[tokio::test]
async fn range_equivalence_hashes_every_byte_of_the_requested_media_slice() {
    let origin = Origin::start().await;
    let request = ready_window(&origin).await;
    let payload = media(1);
    let mut container = vec![0x55; 17];
    container.extend_from_slice(&payload);
    container.extend_from_slice(&[0xaa; 29]);
    *origin.state.bytes.write().await = container;
    let mut fetched = rotate_tail(window(&origin, 101, 4, "old"), "range");
    fetched.body = fetched.body.replace(
        &format!("{}/0123456789abcdef/104.ts?token=range", origin.base),
        &format!("#EXT-X-BYTERANGE:{}@17\n{}/0123456789abcdef/104.ts?token=range", payload.len(), origin.base),
    );
    let original = request.session.read().await.segments[&3].clone();
    assert!(candidate(&request, fetched.clone()).await.is_ok());
    assert_eq!(
        origin.state.transfers.lock().await.last().unwrap().1.as_deref(),
        Some(format!("bytes=17-{}", 17 + payload.len() - 1).as_str())
    );
    assert_eq!(origin.state.transfers.lock().await.last().unwrap().2, payload.len());
    assert!(crate::refresh::origin_probe::commit_pinned_manifest(
        &request,
        &fetched,
        HlsManifestCommitAcceptanceMode::StrictPinnedHost
    )
    .await
    .is_ok());
    assert_eq!(origin.state.transfers.lock().await.len(), 5, "confirmed slice reload has no equality GET");
    let mut invalid = fetched;
    invalid.body = invalid.body.replace(&format!("{}@17", payload.len()), &format!("{}@999999", payload.len()));
    assert!(candidate(&request, invalid).await.is_err());
    assert_eq!(request.session.read().await.segments[&3].origin_fetch_ref, original.origin_fetch_ref);
    assert_eq!(tokio::fs::read(request.segment_cache.object_path(&original.cache_key)).await.unwrap(), payload);
}
