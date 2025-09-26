use chrono::{TimeZone, Utc};
use crate::app::components::NoContent;
use crate::hooks::use_service_context;
use yew::platform::spawn_local;
use yew::prelude::*;

const TIME_BLOCK_WIDTH: f64 = 200.0;

#[function_component]
pub fn EpgView() -> Html {
    let services = use_service_context();
    let epg = use_state(|| None);

    {
        let service_ctx = services.clone();
        let epg_set = epg.clone();
        use_effect_with((), move |_| {
            let service_ctx = service_ctx.clone();
            let epg_set = epg_set.clone();
            spawn_local(async move {
                let playlist_epg = service_ctx.playlist.get_playlist_epg(1).await;
                epg_set.set(playlist_epg);
            });
        });
    }

    if epg.is_none() {
        return html! {
            <div class="tp__epg">
                <NoContent />
            </div>
        };
    }
    let block_size = 30; // Minutes
    let tv = epg.as_ref().unwrap();
    let start_window = (tv.start / 60).max(0);
    let start_window = (start_window / block_size) * block_size; // 30 minutes
    let window_duration = ((tv.stop  / 60) - start_window).max(0);
    let num_blocks = (window_duration + block_size - 1) / block_size;
    let now = Utc::now().timestamp();

    html! {
        <div class="tp__epg">
            <div class="tp__epg__header">
                <div class="tp__epg__channels-header">
                </div>
                <div class="tp__epg__timeline">
                    { for (0..num_blocks).map(|i| {
                        let block_start = start_window + i as i64 * block_size;
                        let block_end = (block_start + block_size).min(start_window + window_duration);
                        let start_time = Utc.timestamp_opt(block_start * 60, 0).unwrap();
                        let end_time = Utc.timestamp_opt(block_end * 60, 0).unwrap();

                        html! {
                            <div class="tp__epg__timeline-block">
                                { start_time.format("%H:%M").to_string() }
                            </div>
                        }
                    }) }
                </div>
            </div>
            <div class="tp__epg__body">
                <div class="tp__epg__channels">
                    { for tv.channels.iter().map(|ch| {
                        html! {
                            <div class="tp__epg__channel">
                                <div class="tp__epg__channel-title">
                                    { &ch.title }
                                </div>
                            </div>
                        }
                      })
                    }
                </div>
                <div class="tp__epg__programs">
                    { for tv.channels.iter().map(|ch| {
                        html! {
                          <div class="tp__epg__channel-programs">
                            { for ch.programmes.iter().map(|p| {
                                let is_active = now >= p.start && now < p.stop;
                                let prog_start_min = (p.start / 60) as i64;
                                let prog_stop_min  = (p.stop / 60) as i64;
                                let rel_start = prog_start_min - start_window;
                                let rel_stop  = prog_stop_min - start_window;
                                let left  = ((rel_start as f64 / 30.0) * TIME_BLOCK_WIDTH).round();
                                let width = (((rel_stop - rel_start) as f64 / 30.0) * TIME_BLOCK_WIDTH).round();
                                html! {
                                <div class={classes!("tp__epg__program", if is_active { "tp__epg__program-active" } else {""})} style={format!("left:{}px; width:{}px; min-width:{}px; max-width:{}px", left, width, width, width)} title={ p.title.clone() }>
                                    { &p.title }
                                </div>
                            }})}
                          </div>
                        }
                      })
                    }
                </div>
            </div>
        </div>
    }
}
