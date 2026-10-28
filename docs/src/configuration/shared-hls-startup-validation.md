# Shared HLS startup reservation: Finding 2 validation

Validated locally on 2026-10-28. The patch grants an explicit, unconfirmed HLS start reservation in the existing provider lease table before
releasing the manifest allocation. The first accepted media manifest fixes the anchor and target duration; authorized media supersedes that
initial protection. Terminal startup failure retains its typed cause per access lease and releases the matching binding without waiting for GC.

The local master retains the existing entry timeout and acquires no provider capacity. A timely session request starts its own bounded
processing phase. No routes or configuration options were added; generic `Starting` leases still do not reserve capacity.

## Acceptance evidence

Tests below are in `backend/app/src/api/endpoints/hls_api/tests/provider_admission_startup_reservation.rs` unless otherwise stated.

| ID | Evidence |
| --- | --- |
| A1 | `master_only_uses_existing_entry_timeout_without_origin_or_provider_admission`: zero allocations and upstream requests; expiry at the existing boundary; unchanged 90-second default. |
| A2 | `timely_session_start_outlives_original_entry_deadline`: a gated successful HTTP manifest crosses the original entry deadline while the processing lease remains valid. `admitted_slow_manifest_can_grant_start_after_ordinary_starting_lease_expired` also checks the expired generic lease with its allocation still live. |
| A3 | `manifest_segment_handoff_preserves_admission_at_limits_one_and_three`: barrier after manifest release and before media preparation, foreign owner denied, original manifest/media delivered. |
| A4 | `start_windows_are_frozen_and_only_media_renews_protection` and `provider_start_is_one_unconfirmed_slot_with_fixed_expiry_and_generation_safe_cleanup`: controlled time proves inclusive hard/soft boundaries at 10/30 seconds and provider expiry at 31 seconds. |
| A5 | `unconfirmed_start_can_be_taken_over_in_soft_window_but_never_during_origin_work`: existing speculative soft-overlap admission succeeds without media confirmation; active origin work blocks it. Existing takeover tests exercise the subsequent handoff. |
| A6 | Fixed-window and handoff tests check polling, additional leases, duration changes and media renewal. `unadmitted_binding_replacement_and_late_polling_preserve_the_start_anchor` checks replacement without admission. `regular_readmission_of_an_expired_start_clears_the_anchor_after_allocation` proves that a fresh real allocation begins a new logical start even without previous media delivery. |
| A7 | Handoff tests combine parallel origin fetching with Pending/Idle joins and count one shared slot. Successful Idle readmission is protected at limits 1 and 3; `idle_reentry_capacity_failure_preserves_previous_publication_and_closes_without_gc` preserves published snapshots and completed playback cursors on failure. |
| A8 | The exhausted-admission test closes after existing bounded resource retries; no lease-expiry or GC step is used. `terminal_start_cleanup_is_immediate_and_cannot_release_a_successor_binding` asserts zero clock advance, no origin work and immediate reservation release. |
| A9 | The exhausted-admission matrix retains default 502, explicitly tests 422/503, enabled missing-asset 503, and enabled provider-exhausted video 200. The finite custom manifest's media is fetched through the actual CVS router. |
| A10 | The same matrix polls the failed lease twice and asserts unchanged origin request counts and logical retry usage. Releasing foreign capacity and opening a fresh master succeeds. |
| A11 | `cancelling_one_waiter_preserves_the_shared_start_for_another_client`, terminal cleanup with a successor generation, and provider expiry/recreation with stale binding tags preserve other users and successor reservations. |
| A12 | Complete session, HLS and backend suites, Finding 1 recovery regressions, and HLS process integrations pass. Local capacity failure remains `ReservedForOther` through the terminal decision and returns the provider-exhausted response rather than 404. |

The Idle test waits for media EOF bookkeeping before taking its retained cursor snapshot. Synchronization uses explicit origin-preparation
barriers and completion evidence. The entry-deadline test deliberately waits across that boundary; competition tests do not rely on sleeps.

## Review corrections

The three follow-up review findings are covered by additional regressions in the same module:

- `pending_and_idle_provisioning_stay_retryable_until_the_job_finishes` gates a real local panel HTTP request. Pending and Idle leases retain
  live provisioning manifests, or a provisional 503 when no media asset exists, across repeated requests. Only the completed failed job marks
  the lease failed. The resolution explicitly distinguishes `Provisioning` from `Exhausted`; response status is not used to infer finality.
- `admitted_idle_reentry_protects_the_manifest_segment_handoff_at_limits_one_and_three` completes playback, expires its old protection and
  obtains a fresh allocation. Its next accepted manifest creates a new anchor before allocation release. A competing owner is rejected at
  the barrier, subsequent polling preserves that anchor and media delivery succeeds.
- `generic_preparation_failures_do_not_select_the_provider_capacity_video` exercises `Unavailable`, `WaitTimedOut` and `AcquireTimedOut`
  through the canonical response path, with a provider-exhausted asset present. They select the channel response instead. Only `Exhausted`
  and `ReservedForOther` qualify as proven capacity denials; their provider video remains available.

An admitted restart is explicit: the prior binding is detached with expired protection or terminal startup failure, and the new allocation
exists. Ordinary binding replacement and polling without a new allocation retain the old anchor. These corrections do not implement the
separate retry/backoff protection task.

## Reproduction and results

Run from the repository root. All tests use the mandated wrapper and `dev-slim` profile. The table records the completed
start-protection baseline; current combined results are in [retry validation](./shared-hls-retry-validation.md):

```sh
./bin/test.sh --offline --profile dev-slim -p tuliprox-session -p tuliprox-hls --lib
./bin/test.sh --offline --profile dev-slim -p tuliprox --bin tuliprox -- --test-threads=4
./bin/test.sh --offline --profile dev-slim -p tuliprox --test hls_origin_account_handover --test hls_revision_recovery
make fmt-check
make markdown-lint
cargo +stable clippy --offline --profile dev-slim -p tuliprox-session -p tuliprox-hls -p tuliprox --all-targets -- -D warnings
```

| Check | Result |
| --- | --- |
| Session library | 387 passed; 1 existing ignored probe |
| HLS library | 1,310 passed; 3 existing ignored probes |
| Backend binary | 1,345 passed in the start-protection baseline |
| HLS process integrations | 1 account-handover test and 3 revision-recovery tests passed |
| Formatting, whitespace, Markdown, strict stable Clippy | Passed |

The profiled direct test and Clippy commands replace unprofiled `make test`/`make lint` as requested. Nightly Clippy was also attempted but
stops in unchanged `shared/src/model/cluster_flags.rs:146` on `clippy::try_from_instead_of_from_str`. Stable Clippy passes with warnings denied;
no unrelated source or lint configuration was changed. macOS test linking reports the existing large `__eh_frame` compact-unwind warning.

## Limits

The actual HLS handler, provider manager, origin HTTP client, cache writes and lease media routes are exercised. The origin is synthetic;
the repeated `0x47` bytes establish successful HTTP transport, not MPEG-TS decoding or real-provider behavior. No production traffic or
provider credentials were used. Disabled-video status and provider-exhausted media coverage use the local HTTP-origin regression matrix;
local tests also cover initial/Idle contested handoff and retained Finding 1 behavior. Provisioning uses an actual,
gated local panel HTTP failure and exercises caller lifecycle, not account creation against a production panel. No push or publication is part
of this work.
