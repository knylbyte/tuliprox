use std::sync::Arc;
use shared::error::TuliproxError;
use shared::model::PlaylistGroup;
use crate::model::{Config, ConfigInput, InputSource};
use crate::processing::parser::m3u;
use crate::utils::prepare_file_path;
use crate::utils::request;

pub async fn get_m3u_playlist(client: Arc<reqwest::Client>, cfg: &Config, input: &ConfigInput, working_dir: &str) -> (Vec<PlaylistGroup>, Vec<TuliproxError>) {
    let input_source: InputSource = {
        match input.staged.as_ref() {
            None => input.into(),
            Some(staged) => staged.into(),
        }
    };
    let persist_file_path = prepare_file_path(input.persist.as_deref(), working_dir, "");
    match request::get_input_text_content(client, &input_source, working_dir, persist_file_path).await {
        Ok(text) => {
            (m3u::parse_m3u(cfg, input, text.lines()), vec![])
        }
        Err(err) => (vec![], vec![err])
    }
}
