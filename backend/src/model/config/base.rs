use std::path::PathBuf;
use path_clean::PathClean;
use shared::error::{TuliproxError};
use shared::model::ConfigDto;
use crate::model::{macros, ConfigApi, ReverseProxyConfig, ScheduleConfig};
use crate::model::{HdHomeRunConfig, IpCheckConfig, LogConfig, MessagingConfig, ProxyConfig, VideoConfig, WebUiConfig};
use crate::utils;

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub threads: u8,
    pub api: ConfigApi,
    pub working_dir: String,
    pub backup_dir: Option<String>,
    pub user_config_dir: Option<String>,
    pub mapping_path: Option<String>,
    pub custom_stream_response_path: Option<String>,
    pub video: Option<VideoConfig>,
    pub schedules: Option<Vec<ScheduleConfig>>,
    pub log: Option<LogConfig>,
    pub user_access_control: bool,
    pub connect_timeout_secs: u32,
    pub sleep_timer_mins: Option<u32>,
    pub update_on_boot: bool,
    pub config_hot_reload: bool,
    pub web_ui: Option<WebUiConfig>,
    pub messaging: Option<MessagingConfig>,
    pub reverse_proxy: Option<ReverseProxyConfig>,
    pub hdhomerun: Option<HdHomeRunConfig>,
    pub proxy: Option<ProxyConfig>,
    pub ipcheck: Option<IpCheckConfig>,
}

impl Config {
    pub fn prepare(&mut self, config_path: &str) -> Result<(), TuliproxError> {
        let work_dir = &self.working_dir;
        self.working_dir = utils::resolve_directory_path(work_dir);

        self.prepare_directories();
        self.prepare_api_web_root();
        if let Some(ref mut webui) = &mut self.web_ui {
            webui.prepare(config_path)?;
        }

        Ok(())
    }


    fn prepare_directories(&mut self) {
        fn set_directory(path: &mut Option<String>, default_subdir: &str, working_dir: &str) {
            *path = Some(match path.as_ref() {
                Some(existing) => existing.to_owned(),
                None => PathBuf::from(working_dir).join(default_subdir).clean().to_string_lossy().to_string(),
            });
        }

        set_directory(&mut self.backup_dir, "backup", &self.working_dir);
        set_directory(&mut self.user_config_dir, "user_config", &self.working_dir);
    }

    fn prepare_api_web_root(&mut self) {
        if !self.api.web_root.is_empty() {
            self.api.web_root = utils::make_absolute_path(&self.api.web_root, &self.working_dir);
        }
    }

}

macros::from_impl!(Config);

impl From<&ConfigDto> for Config {
    fn from(dto: &ConfigDto) -> Self {
        Config {
            threads: 0,
            api: ConfigApi::from(&dto.api),
            working_dir: dto.working_dir.to_string(),
            backup_dir: dto.backup_dir.clone(),
            user_config_dir: dto.user_config_dir.clone(),
            mapping_path: dto.mapping_path.clone(),
            custom_stream_response_path: dto.custom_stream_response_path.clone(),
            video: dto.video.as_ref().map(Into::into),
            schedules: dto.schedules.as_ref().map(|s| s.iter().map(Into::into).collect()),
            log: dto.log.as_ref().map(Into::into),
            user_access_control: dto.user_access_control,
            connect_timeout_secs: dto.connect_timeout_secs,
            sleep_timer_mins: dto.sleep_timer_mins,
            update_on_boot: dto.update_on_boot,
            config_hot_reload: dto.config_hot_reload,
            web_ui: dto.web_ui.as_ref().map(Into::into),
            messaging: dto.messaging.as_ref().map(Into::into),
            reverse_proxy: dto.reverse_proxy.as_ref().map(Into::into),
            hdhomerun: dto.hdhomerun.as_ref().map(Into::into),
            proxy: dto.proxy.as_ref().map(Into::into),
            ipcheck: dto.ipcheck.as_ref().map(Into::into),
        }
    }
}