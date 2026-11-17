use super::*;

async fn fetch_resource(
    fixture: &TestTransientCacheFixture,
    resource: &TransientResourceRef,
    account: &'static str,
) -> Vec<u8> {
    let mut request = fixture.request(None).fetch;
    request.resolved_origin_uri.clone_from(&resource.resolved_origin_uri);
    request.resource_kind = resource.kind;
    request.resource_file =
        TransientResourceFile { resource_id: resource.id.clone(), extension: resource.file_ext_hint.clone().unwrap() };
    request.origin_headers.insert("x-fixture-account", HeaderValue::from_static(account));
    if resource.kind == TransientResourceKind::Key {
        let response = fetch_hls_transient_origin_response_with_attempt_prepare(request, |_| async { Ok(()) }.boxed())
            .await
            .unwrap();
        let mut bytes = Vec::new();
        let mut body = response.decoded.body;
        body.read_to_end(&mut bytes).await.unwrap();
        return bytes;
    }
    let token = {
        let mut session = fixture.session.write().await;
        let proxy = session.proxy_session_id.clone();
        let TransientObjectFetchDecision::Fetch(token) = session.transient.begin_object_fetch(
            &proxy,
            resource,
            &request.resource_file.extension,
            tuliprox_core::utils::current_time_millis(),
            60_000,
        ) else {
            panic!("distinct occurrence starts its own full fetch");
        };
        token
    };
    let key = token.cache_key().clone();
    let mut commit = fixture.request(None).commit;
    commit.resource = resource.clone();
    commit.resource_file = request.resource_file.clone();
    commit.fetch_token = *token;
    fetch_and_commit_hls_transient_origin_response_with_attempt_prepare(
        HlsTransientOriginCacheFetchRequest { fetch: request, commit },
        |_| async { Ok(()) }.boxed(),
    )
    .await
    .unwrap();
    let metadata = fixture.segment_cache.metadata(&key).await.unwrap().unwrap();
    tokio::fs::read(metadata.path).await.unwrap()
}

#[tokio::test]
async fn account_handoff_keeps_old_aes_map_and_ciphertext_bytes_separate_from_identical_new_urls() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/live/index.m3u8", listener.local_addr().unwrap());
    let router = axum::Router::new().fallback(|uri: axum::http::Uri, headers: HeaderMap| async move {
        let marker = if headers.get("x-fixture-account").unwrap() == "old" { 0x11 } else { 0x22 };
        let len = if std::path::Path::new(uri.path())
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("key"))
        {
            16
        } else {
            32
        };
        vec![marker; len]
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let fixture = TestTransientCacheFixture::new(base.clone()).await;
    let now = tuliprox_core::utils::current_time_millis();
    let suffix = "#EXT-X-KEY:METHOD=AES-128,URI=\"shared.key\"\n#EXT-X-MAP:URI=\"shared.mp4\"\n#EXTINF:4,\nshared.ts\n";
    let old = {
        let mut state = fixture.session.write().await;
        let proxy = state.proxy_session_id.clone();
        state
            .transient
            .commit_origin_manifest_for_test(
                &format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n{suffix}"),
                &base,
                &proxy,
                b"rewrite-secret",
                now,
            )
            .unwrap();
        state
            .transient
            .resources
            .values()
            .filter(|resource| resource.resolved_origin_uri != base)
            .cloned()
            .collect::<Vec<_>>()
    };
    let mut old_bytes = Vec::new();
    for resource in &old {
        let bytes = fetch_resource(&fixture, resource, "old").await;
        assert_eq!(bytes, vec![0x11; if resource.kind == TransientResourceKind::Key { 16 } else { 32 }]);
        old_bytes.push((resource.clone(), bytes));
    }
    let current = {
        let mut state = fixture.session.write().await;
        let proxy = state.proxy_session_id.clone();
        state.handoff_origin_account(HlsOriginAccountBinding::rebound(
            Arc::from("input"),
            Arc::from("new-account"),
            "owner".into(),
            1,
            now + 1,
        ));
        state
            .transient
            .commit_origin_manifest_for_test(
                &format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:11\n{suffix}"),
                &base,
                &proxy,
                b"rewrite-secret",
                now + 2,
            )
            .unwrap();
        let ids = state.transient.last_manifest_published_resource_ids();
        state.transient.resources.values().filter(|resource| ids.contains(&resource.id)).cloned().collect::<Vec<_>>()
    };
    assert_eq!(current.len(), 3);
    for resource in &current {
        let (previous, _) = old_bytes.iter().find(|(previous, _)| previous.kind == resource.kind).unwrap();
        assert_eq!(previous.resolved_origin_uri, resource.resolved_origin_uri);
        assert_ne!(previous.id, resource.id);
        assert_eq!(
            fetch_resource(&fixture, resource, "new").await,
            vec![0x22; if resource.kind == TransientResourceKind::Key { 16 } else { 32 }]
        );
    }
    let mut state = fixture.session.write().await;
    let proxy = state.proxy_session_id.clone();
    for (resource, expected) in old_bytes {
        assert_eq!(&state.transient.resources[&resource.id], &resource);
        assert!(!state.transient.origin_resource_is_fetchable(&resource, now + 3));
        if resource.kind != TransientResourceKind::Key {
            let lookup =
                TransientPassthroughState::transient_object_key(&proxy, &resource.id, resource.file_ext_hint.unwrap());
            let entry = state.transient.ready_object(&lookup, resource.kind, now + 3).unwrap();
            let metadata = fixture.segment_cache.metadata(&entry.key).await.unwrap().unwrap();
            assert_eq!(tokio::fs::read(metadata.path).await.unwrap(), expected);
        }
    }
    server.abort();
}
