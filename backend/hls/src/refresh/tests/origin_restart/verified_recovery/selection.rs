use super::*;
use crate::manifest_fetch::OriginManifestFetchError;

async fn recover(
    request: &OriginRefreshRequest,
    origin: &Origin,
    fetched: FetchedOriginManifest,
) -> Result<(), OriginManifestFetchError> {
    *origin.state.fallback_manifest.write().await = Some(fetched.body);
    let context = manifest_fetch_context(request, Some(4));
    let binding = HlsManifestOriginBinding::new(fetched.resolved_request_url.parse().unwrap(), None).unwrap();
    retry_hls_origin_manifest_recovery_chain(
        &context,
        binding,
        None,
        None,
        HlsManifestAcceptanceTrigger::Observe,
        HlsManifestCommitAcceptanceMode::StrictPinnedHost,
        |fetched, mode| super::super::super::super::commit_manifest_recovery_candidate(request, fetched, mode),
    )
    .await
    .map(|_| ())
}

fn tokenized_window(origin: &Origin, count: u64, token: &str) -> FetchedOriginManifest {
    let mut fetched = window(origin, 101, count, "old");
    fetched.body = fetched.body.replace(&origin.base, "").replace("?token=old", "?x=old");
    fetched.body = fetched.body.replace("104.ts?x=old", &format!("104.ts?x={token}"));
    fetched.final_manifest_url.push_str("?token=fixture");
    fetched.resolved_request_url.clone_from(&fetched.final_manifest_url);
    fetched
}

#[tokio::test]
async fn logical_query_change_requires_hash_even_when_fetch_urls_are_identical() {
    use tuliprox_parser::hls::origin_manifest::{parse_origin_media_manifest, OriginManifestParseOutcome};
    let origin = Origin::start().await;
    let mut request = ready_window_from(&origin, tokenized_window(&origin, 4, "old")).await;
    let original = request.session.read().await.segments[&3].clone();
    let mut fetched = tokenized_window(&origin, 5, "old");
    fetched.body = fetched.body.replace("104.ts?x=old", "104.ts?x=old&token=fixture");
    let OriginManifestParseOutcome::Normal(parsed) =
        parse_origin_media_manifest(&fetched.body, &fetched.final_manifest_url)
    else {
        panic!("expected a regular tokenized playlist");
    };
    let segment = parsed.segments.iter().find(|segment| segment.origin_seq == 104).unwrap();
    assert_eq!(segment.resolved_origin_url, original.origin_fetch_ref.as_ref().unwrap().resolved_origin_url);
    assert_eq!(segment.resource_uri, "/0123456789abcdef/104.ts?x=old&token=fixture");
    origin.state.requests.lock().await.clear();
    let result = recover(&request, &origin, fetched.clone()).await;
    assert!(result.is_ok(), "the declared binding still requires its own complete read: {result:?}");
    let proofs = origin.state.requests.lock().await.iter().filter(|uri| uri.contains("104.ts?x=old")).count();
    assert_eq!(proofs, 1);
    reload(&mut request, &origin, fetched).await;
    let state = request.session.read().await;
    assert_eq!(state.segments[&3].origin_fetch_ref, original.origin_fetch_ref);
    assert_eq!(state.segments[&3].occurrence_identity, original.occurrence_identity);
    assert_eq!(HlsOriginContentEvidence::verified_bindings(&state).len(), 1);
    assert_eq!(origin.state.requests.lock().await.iter().filter(|uri| uri.contains("104.ts?x=old")).count(), proofs);
}

#[tokio::test]
async fn inherited_fetch_query_stays_separate_from_the_confirmed_logical_binding() {
    let origin = Origin::start().await;
    let mut request = ready_window_from(&origin, tokenized_window(&origin, 4, "old")).await;
    let (original, revision) = {
        let state = request.session.read().await;
        (state.segments[&3].clone(), state.startup.as_ref().unwrap().revisions[&3].clone())
    };
    let fetched = tokenized_window(&origin, 4, "new");
    let result = recover(&request, &origin, fetched.clone()).await;
    eprintln!(
        "inherited-query recovery: accepted={} candidate_gets={}",
        result.is_ok(),
        origin.state.requests.lock().await.iter().filter(|uri| uri.contains("104.ts?x=new")).count()
    );
    assert!(result.is_ok(), "equal complete HTTP bytes must qualify their parsed URI: {result:?}");
    let verified = origin.state.requests.lock().await.iter().filter(|uri| uri.contains("104.ts?x=new")).count();
    assert_eq!(verified, 1);
    for next in [fetched, tokenized_window(&origin, 5, "new")] {
        reload(&mut request, &origin, next).await;
    }
    {
        let state = request.session.read().await;
        assert_eq!(state.segments[&3].occurrence_identity, original.occurrence_identity);
        assert_eq!(state.segments[&3].origin_fetch_ref, original.origin_fetch_ref);
        assert!(revision.revision().origin_binding_is_current(&state));
        assert_eq!(HlsCacheOriginAnchor::capture(&state).unwrap().identity.origin_msn(), 105);
        assert_eq!(HlsOriginContentEvidence::verified_bindings(&state).len(), 1);
    }
    fetch(&request, 4).await;
    let requests = origin.state.requests.lock().await.clone();
    assert_eq!(requests.iter().filter(|uri| uri.contains("104.ts?x=new")).count(), verified);
    assert!(requests.iter().all(|uri| uri.contains("token=fixture")), "fetch authentication survives every request");
    assert!(requests.iter().any(|uri| uri.contains("105.ts?x=old&token=fixture")));
    assert_eq!(
        tokio::fs::read(revision.revision().wait_complete(tokio::time::Instant::now()).await.unwrap().path)
            .await
            .unwrap(),
        media(1)
    );
}

#[tokio::test]
async fn recovery_selection_reuses_a_confirmed_binding_in_an_advancing_window() {
    for stale in [false, true] {
        let origin = Origin::start().await;
        let request = ready_window(&origin).await;
        assert!(candidate(&request, rotate_tail(window(&origin, 101, 4, "old"), "new")).await.is_ok());
        let original = request.session.read().await.segments[&3].clone();
        if stale {
            request.session.write().await.activity.origin_work_generation += 1;
        }
        origin.state.requests.lock().await.clear();
        let fetched = rotate_tail(window(&origin, 103, 3, "old"), "new");
        let result = recover(&request, &origin, fetched.clone()).await;
        assert!(result.is_ok(), "a confirmed occurrence with a new tail remains selectable: {result:?}");
        let requests = origin.state.requests.lock().await.clone();
        assert_eq!(
            requests.iter().filter(|uri| uri.contains("104.ts?token=new")).count(),
            usize::from(stale),
            "stale authority requires a fresh complete hash"
        );
        assert!(
            requests.iter().filter(|uri| path_has_extension(uri, "m3u8")).count() > 1,
            "the real bounded recovery burst ran"
        );
        let state = request.session.read().await;
        assert_eq!(state.origin_epoch, 0);
        assert_eq!(state.proxy_next_seq, Some(5));
        assert_eq!(state.segments[&3].occurrence_identity, original.occurrence_identity);
        assert_eq!(state.segments[&3].origin_fetch_ref, original.origin_fetch_ref);
        assert_eq!(HlsCacheOriginAnchor::capture(&state).unwrap().identity.origin_msn(), 105);
        drop(state);
        assert!(crate::refresh::origin_probe::commit_pinned_manifest(
            &request,
            &fetched,
            HlsManifestCommitAcceptanceMode::StrictPinnedHost
        )
        .await
        .is_ok());
        assert_eq!(*origin.state.requests.lock().await, requests);
    }
}

#[tokio::test]
async fn advancing_rotation_reaches_full_hash_verification_without_bypassing_negative_gates() {
    for case in ["equal", "mirror", "changed", "missing", "dependency", "order"] {
        let origin = Origin::start().await;
        let request = ready_window(&origin).await;
        let original = request.session.read().await.segments.clone();
        let mut fetched = rotate_tail(window(&origin, 103, 3, "old"), "new");
        let expected_success = matches!(case, "equal" | "mirror");
        match case {
            "mirror" => {
                fetched.final_manifest_url = fetched.final_manifest_url.replace("127.0.0.1", "localhost");
                fetched.resolved_request_url.clone_from(&fetched.final_manifest_url);
                fetched.redirect_host = Some("localhost".into());
            }
            "changed" => {
                let mut changed = media(1);
                *changed.last_mut().unwrap() ^= 1;
                *origin.state.bytes.write().await = changed;
            }
            "missing" => request.session.write().await.origin_content = HlsOriginContentEvidence::default(),
            "dependency" => fetched.body = fetched.body.replace("#EXT-X-TARGETDURATION:4", "#EXT-X-TARGETDURATION:4\n#EXT-X-KEY:METHOD=AES-128,URI=\"new-key.bin\",IV=0x00000000000000000000000000000001"),
            "order" => fetched = rotate_tail(window(&origin, 100, 6, "old"), "new"),
            _ => {}
        }
        origin.state.requests.lock().await.clear();
        let result = recover(&request, &origin, fetched).await;
        assert_eq!(result.is_ok(), expected_success, "{case}: {result:?}");
        let state = request.session.read().await;
        assert_eq!(state.origin_epoch, 0);
        assert_eq!(state.segments[&3].occurrence_identity, original[&3].occurrence_identity);
        assert_eq!(state.segments[&3].origin_fetch_ref, original[&3].origin_fetch_ref);
        if expected_success {
            assert_eq!(state.proxy_next_seq, Some(5));
            assert_eq!(HlsOriginContentEvidence::verified_bindings(&state).len(), 1);
        } else {
            assert_eq!(state.segments, original);
            assert_eq!(HlsOriginContentEvidence::verified_bindings(&state), []);
        }
        let requests = origin.state.requests.lock().await;
        let proofs = requests.iter().filter(|uri| uri.contains("104.ts?token=new")).count();
        match case {
            "equal" | "mirror" => assert_eq!(proofs, 1),
            "changed" => assert!(proofs > 0, "a full real candidate response was compared"),
            _ => assert_eq!(proofs, 0, "missing provenance or dependency rejection precedes hash I/O"),
        }
    }
}
