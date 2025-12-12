use crate::api::model::AppState;
use crate::messaging::send_message;
use crate::model::{is_input_expired, Config, ConfigInput, ConfigTarget, XtreamLoginInfo};
use crate::model::{InputSource, ProxyUserCredentials};
use crate::processing::parser::xtream;
use crate::repository::xtream_repository;
use crate::repository::xtream_repository::{rewrite_xtream_series_info_content, rewrite_xtream_vod_info_content, xtream_get_input_info};
use crate::utils::request;
use chrono::{DateTime, Utc};
use log::{error, info, warn};
use shared::error::{str_to_io_error, TuliproxError};
use shared::model::{MsgKind, PlaylistEntry, PlaylistGroup, ProxyUserStatus, XtreamCluster, XtreamPlaylistItem};
use shared::utils::{extract_extension_from_url, get_i64_from_serde_value, get_string_from_serde_value};
use std::io::Error;
use std::str::FromStr;
use std::sync::Arc;

const THREE_DAYS_IN_SECS: i64 = 3 * 24 * 60 * 60;

#[inline]
pub fn get_xtream_stream_url_base(url: &str, username: &str, password: &str) -> String {
    format!("{url}/player_api.php?username={username}&password={password}")
}

pub fn get_xtream_player_api_action_url(input: &ConfigInput, action: &str) -> Option<String> {
    if let Some(user_info) = input.get_user_info() {
        Some(format!("{}&action={}",
                     get_xtream_stream_url_base(
                         &user_info.base_url,
                         &user_info.username,
                         &user_info.password),
                     action
        ))
    } else {
        None
    }
}

pub fn get_xtream_player_api_info_url(input: &ConfigInput, cluster: XtreamCluster, stream_id: u32) -> Option<String> {
    let (action, stream_id_field) = match cluster {
        XtreamCluster::Live => (crate::model::XC_ACTION_GET_LIVE_INFO, crate::model::XC_LIVE_ID),
        XtreamCluster::Video => (crate::model::XC_ACTION_GET_VOD_INFO, crate::model::XC_VOO_ID),
        XtreamCluster::Series => (crate::model::XC_ACTION_GET_SERIES_INFO, crate::model::XC_SERIES_ID),
    };
    get_xtream_player_api_action_url(input, action).map(|action_url| format!("{action_url}&{stream_id_field}={stream_id}"))
}


pub async fn get_xtream_stream_info_content(client: &reqwest::Client, input: &InputSource) -> Result<String, Error> {
    match request::download_text_content(client, None, input, None, None).await {
        Ok((content, _response_url)) => Ok(content),
        Err(err) => Err(err)
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn get_xtream_stream_info<P>(client: &reqwest::Client,
                                       app_state: &Arc<AppState>,
                                       user: &ProxyUserCredentials,
                                       input: &ConfigInput,
                                       target: &ConfigTarget,
                                       pli: &P,
                                       info_url: &str,
                                       cluster: XtreamCluster) -> Result<String, Error>
where
    P: PlaylistEntry,
{
    let xtream_output = target.get_xtream_output().ok_or_else(|| Error::other("Unexpected error, missing xtream output"))?;

    let app_config = &app_state.app_config;

    if cluster == XtreamCluster::Series {
        if let Some(content) = xtream_repository::xtream_load_series_info(app_config, target.name.as_str(), pli.get_virtual_id()).await {
            // Deliver existing target content
            return rewrite_xtream_series_info_content(app_state, target, xtream_output, pli, user, &content).await;
        }

        // Check if the content has been resolved
        if xtream_output.resolve_series {
            if let Some(provider_id) = pli.get_provider_id() {
                if let Some(content) = xtream_get_input_info(app_config, input, provider_id, XtreamCluster::Series).await {
                    return xtream_repository::write_and_get_xtream_series_info(app_state, target, xtream_output, pli, user, &content).await;
                }
            }
        }
    } else if cluster == XtreamCluster::Video {
        if let Some(content) = xtream_repository::xtream_load_vod_info(app_config, target.name.as_str(), pli.get_virtual_id()).await {
            // Deliver existing target content
            return rewrite_xtream_vod_info_content(app_config, target, xtream_output, pli, user, &content);
        }
        // Check if the content has been resolved
        if xtream_output.resolve_vod {
            if let Some(provider_id) = pli.get_provider_id() {
                if let Some(content) = xtream_get_input_info(app_config, input, provider_id, XtreamCluster::Video).await {
                    return xtream_repository::write_and_get_xtream_vod_info(app_config, target, xtream_output, pli, user, &content).await;
                }
            }
        }
    }

    let input_source = InputSource::from(input).with_url(info_url.to_owned());
    if let Ok(content) = get_xtream_stream_info_content(client, &input_source).await {
        return match cluster {
            XtreamCluster::Live => Ok(content),
            XtreamCluster::Video => xtream_repository::write_and_get_xtream_vod_info(app_config, target, xtream_output, pli, user, &content).await,
            XtreamCluster::Series => xtream_repository::write_and_get_xtream_series_info(app_state, target, xtream_output, pli, user, &content).await,
        };
    }

    Err(str_to_io_error(&format!("Cant find stream with id: {}/{}/{}",
                                 target.name.replace(' ', "_").as_str(), &cluster, pli.get_virtual_id())))
}

fn get_skip_cluster(input: &ConfigInput) -> Vec<XtreamCluster> {
    let mut skip_cluster = vec![];
    if let Some(input_options) = &input.options {
        if input_options.xtream_skip_live {
            skip_cluster.push(XtreamCluster::Live);
        }
        if input_options.xtream_skip_vod {
            skip_cluster.push(XtreamCluster::Video);
        }
        if input_options.xtream_skip_series {
            skip_cluster.push(XtreamCluster::Series);
        }
    }
    if skip_cluster.len() == 3 {
        info!("You have skipped all sections from xtream input {}", &input.name);
    }
    skip_cluster
}

const ACTIONS: [(XtreamCluster, &str, &str); 3] = [
    (XtreamCluster::Live, crate::model::XC_ACTION_GET_LIVE_CATEGORIES, crate::model::XC_ACTION_GET_LIVE_STREAMS),
    (XtreamCluster::Video, crate::model::XC_ACTION_GET_VOD_CATEGORIES, crate::model::XC_ACTION_GET_VOD_STREAMS),
    (XtreamCluster::Series, crate::model::XC_ACTION_GET_SERIES_CATEGORIES, crate::model::XC_ACTION_GET_SERIES)];

async fn xtream_login(cfg: &Config, client: &reqwest::Client, input: &InputSource, username: &str) -> Result<Option<XtreamLoginInfo>, TuliproxError> {
    let content = if let Ok(content) = request::get_input_json_content(client, None, input, None).await {
        content
    } else {
        let input_source_account_info = input.with_url(format!("{}&action={}", &input.url, crate::model::XC_ACTION_GET_ACCOUNT_INFO));
        match request::get_input_json_content(client, None, &input_source_account_info, None).await {
            Ok(content) => content,
            Err(err) => {
                warn!("Failed to login xtream account {username} {err}");
                return Err(err);
            }
        }
    };

    let mut login_info = XtreamLoginInfo {
        status: None,
        exp_date: None,
    };

    if let Some(user_info) = content.get("user_info") {
        if let Some(status_value) = user_info.get("status") {
            if let Some(status) = get_string_from_serde_value(status_value) {
                if let Ok(cur_status) = ProxyUserStatus::from_str(&status) {
                    login_info.status = Some(cur_status);
                    if !matches!(cur_status, ProxyUserStatus::Active | ProxyUserStatus::Trial) {
                        warn!("User status for user {username} is {cur_status:?}");
                        send_message(client, MsgKind::Info, cfg.messaging.as_ref(), &format!("User status for user {username} is {cur_status:?}")).await;
                    }
                }
            }
        }

        if let Some(exp_value) = user_info.get("exp_date") {
            if let Some(expiration_timestamp) = get_i64_from_serde_value(exp_value) {
                login_info.exp_date = Some(expiration_timestamp);
                notify_account_expire(login_info.exp_date, cfg, client, username, &input.name).await;
            }
        }
    }

    if login_info.exp_date.is_none() && login_info.status.is_none() {
        Ok(None)
    } else {
        Ok(Some(login_info))
    }
}

pub async fn notify_account_expire(exp_date: Option<i64>, cfg: &Config, client: &reqwest::Client, username: &str, input_name: &str) {
    if let Some(expiration_timestamp) = exp_date {
        let now_secs = Utc::now().timestamp(); // UTC-Time
        if expiration_timestamp > now_secs {
            let time_left = expiration_timestamp - now_secs;

            if time_left < THREE_DAYS_IN_SECS {
                if let Some(datetime) = DateTime::<Utc>::from_timestamp(expiration_timestamp, 0) {
                    let formatted = datetime.format("%Y-%m-%d %H:%M:%S").to_string();
                    warn!("User account for user {username} expires {formatted}");
                    send_message(client, MsgKind::Info, cfg.messaging.as_ref(), &format!("User account for user {username} expires {formatted}")).await;
                }
            }
        } else {
            warn!("User account for user {username} is expired");
            send_message(client, MsgKind::Info, cfg.messaging.as_ref(), &format!("User account for user {username} for provider {input_name} is expired")).await;
        }
    }
}

pub async fn get_xtream_playlist(cfg: &Arc<Config>, client: &reqwest::Client, input: &Arc<ConfigInput>, working_dir: &str) -> (Vec<PlaylistGroup>, Vec<TuliproxError>) {
    let input_source: InputSource = {
        match input.staged.as_ref() {
            None => input.as_ref().into(),
            Some(staged) => staged.into(),
        }
    };

    let username = input_source.username.as_ref().map_or("", |v| v);
    let password = input_source.password.as_ref().map_or("", |v| v);

    let base_url = get_xtream_stream_url_base(&input_source.url, username, password);
    let input_source_login = input_source.with_url(base_url.clone());

    check_alias_user_state(cfg, client, input).await;

    if let Err(err) = xtream_login(cfg, client, &input_source_login, username).await {
        error!("Could not log in with xtream user {username} for provider {}. {err}", input.name);
        return (Vec::with_capacity(0), vec![err]);
    }

    let mut playlist_groups: Vec<PlaylistGroup> = Vec::with_capacity(128);
    let skip_cluster = get_skip_cluster(input);

    let mut errors = vec![];
    for (xtream_cluster, category, stream) in &ACTIONS {
        if !skip_cluster.contains(xtream_cluster) {
            let input_source_category = input_source.with_url(format!("{base_url}&action={category}"));
            let input_source_stream = input_source.with_url(format!("{base_url}&action={stream}"));
            let category_file_path = crate::utils::prepare_file_path(input.persist.as_deref(), working_dir, format!("{category}_").as_str());
            let stream_file_path = crate::utils::prepare_file_path(input.persist.as_deref(), working_dir, format!("{stream}_").as_str());

            match futures::join!(
                request::get_input_json_content_as_stream(client, None, &input_source_category, category_file_path),
                request::get_input_json_content_as_stream(client, None, &input_source_stream, stream_file_path)
            ) {
                (Ok(category_content), Ok(stream_content)) => {
                    match xtream::parse_xtream(input,
                                               *xtream_cluster,
                                               category_content,
                                               stream_content).await {
                        Ok(sub_playlist_parsed) => {
                            if let Some(mut xtream_sub_playlist) = sub_playlist_parsed {
                                playlist_groups.append(&mut xtream_sub_playlist);
                            }
                        }
                        Err(err) => errors.push(err)
                    }
                }
                (Err(err1), Err(err2)) => {
                    errors.extend([err1, err2]);
                }
                (_, Err(err)) | (Err(err), _) => errors.push(err),
            }
        }
    }

    for (grp_id, plg) in (1_u32..).zip(playlist_groups.iter_mut()) {
        plg.id = grp_id;
    }
    (playlist_groups, errors)
}

async fn check_alias_user_state(cfg: &Arc<Config>, client: &reqwest::Client, input: &Arc<ConfigInput>) {
    if let Some(aliases) = input.aliases.as_ref() {
        for alias in aliases {
            if is_input_expired(alias.exp_date) {
                notify_account_expire(alias.exp_date, cfg, client, alias.username.as_ref().map_or("", |s| s.as_str()), &alias.name).await;
            }
        }
    }

    // TODO figure out how and when to call it to avoid provider bans. Possible reason for provider ban is to avoid brute force attacks.

    //
    // let cfg = Arc::clone(cfg);
    // let client = Arc::clone(client);
    // let input = Arc::clone(input);
    //
    // tokio::spawn(async move {
    //     for alias in &aliases {
    //         // Random wait time  60–180 seconds to avoid provider block
    //         let delay = u64::from(fastrand::u32(60..=180));
    //         tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
    //
    //         if let (Some(username), Some(password)) =
    //             (alias.username.as_ref(), alias.password.as_ref())
    //         {
    //             let mut input_source: InputSource = input.as_ref().into();
    //             input_source.username.clone_from(&alias.username);
    //             input_source.password.clone_from(&alias.password);
    //             input_source.url.clone_from(&alias.url);
    //             let base_url = get_xtream_stream_url_base(
    //                 &input_source.url,
    //                 username,
    //                 password,
    //             );
    //             let input_source_login = input_source.with_url(base_url.clone());
    //
    //             match xtream_login(&cfg, &client, &input_source_login, username).await {
    //                 Ok(Some(xtream_login_info)) => {
    //                     // TODO need to update the alias
    //
    //                 }
    //                 Ok(None) => error!("Could log in with xtream user {} for provider {}. But could not extract account info", username, alias.name),
    //                 Err(err) => error!("Could not log in with xtream user {} for provider {}. {err}",username,alias.name),
    //             }
    //         }
    //     }
    // });
}

pub fn create_vod_info_from_item(target: &ConfigTarget, user: &ProxyUserCredentials, pli: &XtreamPlaylistItem, last_updated: i64) -> String {
    let category_id = pli.category_id;
    let stream_id = if user.proxy.is_redirect(pli.item_type) || target.is_force_redirect(pli.item_type) { pli.provider_id } else { pli.virtual_id };
    let name = &pli.name;
    let extension = pli.get_additional_property("container_extension")
        .map_or_else(|| extract_extension_from_url(&pli.url).map_or_else(String::new, std::string::ToString::to_string),
                     |v| get_string_from_serde_value(&v).unwrap_or_default());
    let added = last_updated / 1000;
    format!(r#"{{
  "info": {{}},
  "movie_data": {{
    "added": "{added}",
    "category_id": {category_id},
    "category_ids": [{category_id}],
    "container_extension": "{extension}",
    "custom_sid": "",
    "direct_source": "",
    "name": "{name}",
    "stream_id": {stream_id}
  }}
}}"#)
}
