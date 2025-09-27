use std::rc::Rc;
use chrono::{Datelike, Local, TimeZone, Utc};
use gloo_timers::callback::Interval;
use web_sys::HtmlElement;
use crate::app::components::{Breadcrumbs, NoContent, PlaylistSourceSelector};
use crate::hooks::use_service_context;
use yew::platform::spawn_local;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{EpgTv, PlaylistRequest, PlaylistRequestType};
use crate::model::{BusyStatus, EventMessage, ExplorerSourceType};

const TIME_BLOCK_WIDTH: f64 = 210.0;
const TIME_BLOCK_MINS: i64 = 30;
const PIXEL_PER_MIN:f64 = TIME_BLOCK_WIDTH as f64 / TIME_BLOCK_MINS as f64;

fn get_pos(secs: i64, start_mins: i64) -> i64 {
    let mins = secs / 60;
    let rel_mins = mins - start_mins;
    (rel_mins as f64 * PIXEL_PER_MIN).round() as i64
}

#[function_component]
pub fn EpgView() -> Html {
    let services = use_service_context();
    let translate = use_translation();
    let epg = use_state::<Option<EpgTv>, _>(|| None);
    let breadcrumbs = use_state(|| Rc::new(vec![translate.t("LABEL.PLAYLISTS"), translate.t("LABEL.PLAYLIST_EPG")]));
    let container_ref = use_node_ref();
    let now_line_ref = use_node_ref();

    let handle_select_source = {
        let service_ctx = services.clone();
        let epg_set = epg.clone();
        Some(Callback::from(move |req: PlaylistRequest| {
            epg_set.set(None);
            if req.rtype == PlaylistRequestType::Target {
                if let Some(target_id) = req.source_id {
                    let service_ctx = service_ctx.clone();
                    let epg_set = epg_set.clone();
                    service_ctx.event.broadcast(EventMessage::Busy(BusyStatus::Show));
                    spawn_local(async move {
                        let playlist_epg = service_ctx.playlist.get_playlist_epg(target_id).await;
                        epg_set.set(playlist_epg);
                        service_ctx.event.broadcast(EventMessage::Busy(BusyStatus::Hide));
                    });
                }
            }
        }))
    };

    {
        let container_ref = container_ref.clone();
        let now_line_ref = now_line_ref.clone();
        use_effect_with(epg.clone(), move |epg_tv| {
            // TODO  Active elements color change after time update
            let epg_tv_clone = epg_tv.clone();

            let calculate_position = Rc::new(move |epg_tv: &UseStateHandle<Option<EpgTv>>|  {
                if let Some(tv) = &**epg_tv {
                    if let (Some(div), Some(now_line)) = (container_ref.cast::<HtmlElement>(), now_line_ref.cast::<HtmlElement>()) {
                        let start_window_secs = (tv.start / (TIME_BLOCK_MINS*60)) * (TIME_BLOCK_MINS*60);
                        let start_window =  (start_window_secs / 60).max(0);
                        let now = Utc::now().timestamp();
                        let now_line_pos = get_pos(now, start_window);
                        let container_width = div.client_width();
                        let scroll_pos = (now_line_pos as i32 - (container_width >> 1)).max(0);
                        div.set_scroll_left(scroll_pos);
                        now_line.style().set_property("left", &format!("{now_line_pos}px")).unwrap();
                    }
                }
            });

            let calculate_pos = calculate_position.clone();
            let interval = Interval::new(60_000, move || {
                calculate_pos(&epg_tv_clone);
            });
            calculate_position(epg_tv);
            || drop(interval)
        });
    }

    html! {
        <div class="tp__epg tp__list-view">
            <Breadcrumbs items={&*breadcrumbs}/>
            <div class="tp__epg__header">
                <h1>{translate.t("LABEL.PLAYLIST_EPG")}</h1>
            </div>
            <PlaylistSourceSelector hide_title={true} source_types={Some(vec![ExplorerSourceType::Hosted])} on_select={handle_select_source} />
            <div class="tp__epg__body" ref={container_ref} >
                {
                    if epg.is_none() {
                        html! {
                            <NoContent />
                        }
                   } else {
                        let tv = epg.as_ref().unwrap();
                        let start_window_secs = (tv.start / (TIME_BLOCK_MINS*60)) * (TIME_BLOCK_MINS*60);
                        let start_window =  (start_window_secs / 60).max(0);
                        let end_window = (tv.stop / 60).max(0);
                        let window_duration = (end_window - start_window).max(0);
                        let num_blocks = (window_duration + TIME_BLOCK_MINS - 1) / TIME_BLOCK_MINS;
                        let now = Utc::now().timestamp();
                        let now_line_pos = get_pos(now, start_window);
                        let block_style= format!("width:{TIME_BLOCK_WIDTH}px; min-width:{TIME_BLOCK_WIDTH}px; max-width:{TIME_BLOCK_WIDTH}px");
                     html! {
                        <>
                        <div class="tp__epg__channels">
                            <div class="tp__epg__channels-header">
                            </div>
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
                            <div class="tp__epg__timeline">
                                { for (0..num_blocks).map(|i| {
                                    let block_start = start_window + i as i64 * TIME_BLOCK_MINS;
                                    let start_time_utc = Utc.timestamp_opt(block_start * 60, 0).unwrap();
                                    let start_time_local = start_time_utc.with_timezone(&Local);
                                    let hour_min = start_time_local.format("%H:%M").to_string();
                                    let day_month = format!("{:02}.{:02}", start_time_local.day(), start_time_local.month());
                                    html! {
                                        <div class="tp__epg__timeline-block" style={block_style.clone()}>
                                            <div class="tp__epg__timeline-block-time">{ hour_min }</div>
                                            <div class="tp__epg__timeline-block-date">{ day_month }</div>
                                        </div>
                                    }
                                }) }
                            </div>

                            { for tv.channels.iter().map(|ch| {
                                html! {
                                  <div class="tp__epg__channel-programs">
                                    { for ch.programmes.iter().map(|p| {
                                        let is_active = now >= p.start && now < p.stop;
                                        let left = get_pos(p.start, start_window);
                                        let right = get_pos(p.stop, start_window);
                                        let width = right - left;

                                        let pstart_time = Utc.timestamp_opt(p.start, 0).unwrap();
                                        let pend_time = Utc.timestamp_opt(p.stop, 0).unwrap();
                                        let pstart_time_local = pstart_time.with_timezone(&Local);
                                        let pend_time_local = pend_time.with_timezone(&Local);
                                        let pstart = pstart_time_local.format("%H:%M").to_string();
                                        let pend = pend_time_local.format("%H:%M").to_string();
                                        let program_style = format!("left:{left}px; width:{width}px; min-width:{width}px; max-width:{width}px");

                                        html! {
                                        <div class={classes!("tp__epg__program", if is_active { "tp__epg__program-active" } else {""})} style={program_style} title={ p.title.clone() }>
                                            <div class="tp__epg__program-time">{ &pstart } {"-"} { &pend }</div>
                                            <div class="tp__epg__program-title">{ &p.title }</div>
                                        </div>
                                    }})}
                                  </div>
                                }
                              })
                            }
                            <div ref={now_line_ref} class="tp__epg__now-line" style={format!("left:{now_line_pos}px")}></div>
                        </div>
                        </>
                     }
                   }
                }
            </div>
        </div>
    }
}
