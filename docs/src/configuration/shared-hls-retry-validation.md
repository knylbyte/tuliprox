# Shared HLS retry capacity validation

## Scope and implementation

The second Finding 2 assignment protects an admitted shared **live HLS** resource operation beyond normal `3T + 1s`
protection. The completed start protection, Finding 1 recovery, entry deadlines, normal HardActive/SoftActive windows,
TS, DASH and Catchup behavior remain in place. There are no new production HTTP routes or configuration options.

One `HlsOriginRetryWork` owns the operation's immutable deadline, origin-work counter and identified provider work right.
It starts before resource preparation and spans retries, backoff and required processing. Segment plus required AES key
use the existing combined workload budget; standalone MAP/key/transient operations use their resource budgets. Every
attempt retains its individual limits and cannot run beyond the remaining overall budget. Preparation and commit
callbacks are inside that bound. Direct transient responses retain the operation in the response body through EOF,
timeout or Drop, including when the downstream body is not polled. Exposed bytes never initiate another complete retry.

The existing `ProviderSlotLease` stores work IDs and deadlines. Its effective expiry is the maximum of normal protection
and live work deadlines; allocation, normal reservation and work rights still represent one slot. A right requires an own
admitted allocation or valid normal reservation and the exact binding tag. Another operation's expired normal protection
cannot authorize a new independent operation, including when its allocation is temporarily reused. Each guard removes
only its own right. Deadline indexing releases capacity even if a worker retains its guard; a weak HLS timer also releases
its work counter. Cleanup and cancellation use the same synchronous, exactly-once transition and cannot release a successor.

Only authorized media activity renews normal protection. Polls, additional viewers, retries, target-duration changes and
configuration changes cannot renew a running operation's deadline. Definitive startup failure uses the existing typed
failure path; a single operation's failure leaves other work and retained publications intact.

Two existing feature-gated enum imports were corrected so the default production build also compiles. This changes no
admission or recovery behavior. The new synchronization barrier is compiled only for tests or `test-support`.

## Acceptance evidence

All test names below are reproducible with the commands in the next section. Controlled Tokio time and explicit retry
barriers replace timing races. The backend helper `assert_retry_http_lifecycle` starts its own local Rust HTTP origin.
The tests require no external test runner or configured origin endpoint.

| ID | Evidence |
| --- | --- |
| R1 | Timeout, backoff and follow-up preparation keep the same HLS work generation, account generation and provider binding tag. The backend checks both paused phases and attempts automatic expiry detachment. |
| R2 | Target duration is two seconds; the normal reservation is seven seconds. A ten-second first timeout precedes backoff in the controlled Rust HTTP test. Capacity remains reserved. |
| R3 | Both limits 1 and 3 occupy all other slots. Competitor admission fails during transfer and both retry barriers. Backoff has no own I/O allocation; actual foreign allocations plus this reservation equal the limit. |
| R4 | `work_rights_require_own_admission_and_cannot_chain_expired_normal_rights` rejects foreign ownership, stale tags and chained independent rights after normal expiry, including a reused allocation. |
| R5 | The second request succeeds and the lease media route returns the exact 8,192 bytes. Work/account generations and binding tag remain unchanged, with no artificial media confirmation on origin success. |
| R6 | The exhausted-retry case and existing permanent-404 startup regressions close the startup attempt, leave zero workers/counters and release capacity immediately. Same-lease repoll creates no origin requests. Hanging callbacks end at the fixed deadline and release handles through the cleanup lane. |
| R7 | Lifecycle tests abort preparation, transfer and backoff after normal expiry and assert zero counters, rights and I/O allocations. The backend cancels one waiter in backoff; another viewer completes the same shared fetch. |
| R8 | Parallel provider rights release independently; late cleanup preserves a successor tag. Direct-body failure retains another operation's work and capacity. |
| R9 | A retained, unpolled guard expires despite additional real access leases, manifest activity and timeout/target-duration changes. A hanging commit ends at the original deadline; subsequent Drop does not decrement twice. |
| R10 | Segment, MAP and key operations exercise the common retry/deadline contract for normal and transient sources. Nine direct segment/MAP/key cases cover EOF, Drop and deadline with a real provider manager; each makes one origin request and releases its own handle/right. Existing AES/cache-body tests remain green. |
| R11 | The final-failure HTTP scenario closes through the existing failure response and immediately admits a competitor. Direct-body failure preserves parallel work. Existing Idle reentry/publication and startup-recovery regressions check retained usable windows. |
| R12 | Full Session/HLS/backend regressions include start protection, Finding 1, SoftActive takeover, account handoff, stale workers, generations and cleanup. The process integration tests also pass with their own local Rust origins. |

## Commands and results

Run from the repository root:

```sh
CARGO_INCREMENTAL=0 ./bin/test.sh --offline --profile dev-slim -p tuliprox-session -p tuliprox-hls --lib
CARGO_INCREMENTAL=0 ./bin/test.sh --offline --profile dev-slim -p tuliprox --bin tuliprox -- --test-threads=4
CARGO_INCREMENTAL=0 ./bin/test.sh --offline --profile dev-slim -p tuliprox --test hls_origin_account_handover --test hls_revision_recovery
CARGO_INCREMENTAL=0 cargo +stable check --offline --profile dev-slim -p tuliprox-hls -p tuliprox
CARGO_INCREMENTAL=0 cargo +stable clippy --offline --profile dev-slim -p tuliprox-session -p tuliprox-hls -p tuliprox --all-targets -- -D warnings
make fmt-check
make markdown-lint
```

| Check | Result |
| --- | --- |
| Session library | 387 passed; 1 existing ignored probe |
| HLS library | 1,315 passed; 3 existing ignored probes |
| Backend binary | 1,350 passed; 2 existing ignored probes |
| Process integrations | 1 account-handover and 3 revision-recovery tests passed |
| Default production build | Passed |
| Strict stable Clippy | Passed with warnings denied |
| Formatting / Markdown | Passed; checked again after final documentation updates |

The profiled direct test and Clippy commands replace unprofiled `make test`/`make lint` to honor the required `dev-slim`
profile. Stable Clippy is used as in the completed first assignment: the pinned nightly reports an unrelated existing
lint in `shared/src/model/cluster_flags.rs`. Incremental compilation is disabled after a local rustc incremental
fingerprint error; no source workaround or unrelated lint exemption was added. macOS test linking reports the existing
large `__eh_frame` compact-unwind warning.

The native Rust HTTP fixture has one media candidate, target duration two seconds and the existing `FirstReady` mode.
Each test starts its own loopback origin. The first segment response remains gated while controlled Tokio time advances
through its ten-second transfer timeout. Later attempts return either HTTP 200 with 8,192 bytes or HTTP 503 through
attempt exhaustion. The first barrier waits for Hyper's connection handoff before advancing time. Further barriers
observe backoff and follow-up preparation with no own provider I/O handle and a still-reserved logical slot.

The assertions check actual origin request counts, capacity, generations, cleanup, configured failure status and media
delivery at limits 1 and 3. No endpoint override or external test service is used. The precreated session receives its
regular Processing phase once before the request, allowing the test to install synchronization barriers.

Temporary Rust validation logs are `/tmp/tuliprox-retry-{core-verified,backend-verified,integration-verified,
clippy-verified,default-check,fmt-check,markdown}.log`. The removal follow-up is verified in
`/tmp/tuliprox-native-backend.log` (backend and process integrations).

## Changed files and limits

The retry changes cover provider leases and their manager; HLS resource work, retry core, budgets and segment/MAP/key
fetchers; direct transient delivery and app preparation; test-only retry barriers; the backend regression module;
operator documentation and self-contained Rust HTTP fixtures. New sources are `resource_fetch/work.rs`,
`resource_fetch/tests/retry_work.rs`, `transient_fetcher/tests/retry_work.rs` and
`hls_api/tests/provider_admission_retry_reservation.rs`, plus this report. Existing staged first-assignment work is preserved.

The HLS handler, provider manager, HTTP client, cache and lease media routes run for real against local Rust origins.
Synthetic TS bytes prove HTTP transport and state transitions, not MPEG-TS decoding or production provider behavior.
The change addresses the demonstrated retry protection gap; it does not claim one common cause for
all historical `StoreRace` or `Unavailable` events. No production provider calls, credentials, push or publication are used.
