use std::fs::File;
use env_logger::{Builder, Target};
use log::{error, info, LevelFilter};
use crate::model::LogLevelConfig;
use crate::utils::config_file_reader;

const LOG_ERROR_LEVEL_MOD: &[&str] = &[
    "reqwest::async_impl::client",
    "reqwest::connect",
    "hyper_util::client",
];


fn get_log_level(log_level: &str) -> LevelFilter {
    match log_level.to_lowercase().as_str() {
        "trace" => LevelFilter::Trace,
        "debug" => LevelFilter::Debug,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        // "info" => LevelFilter::Info,
        _ => LevelFilter::Info,
    }
}

pub fn init_logger(user_log_level: Option<&String>, config_file: &str) {
    let env_log_level = std::env::var("TULIPROX_LOG").ok();

    let mut log_builder = Builder::from_default_env();
    log_builder.target(Target::Stdout);

    // priority  CLI-Argument, Env-Var, Config, Default
    let log_level = user_log_level
        .map(std::string::ToString::to_string) // cli-argument
        .or(env_log_level) // env
        .or_else(|| {               // config
            File::open(config_file).ok()
                .and_then(|file| serde_yaml::from_reader::<_, LogLevelConfig>(config_file_reader(file, true))
                    .map_err(|e| error!("Failed to parse log config file: {e}"))
                    .ok())
                .and_then(|cfg| cfg.log.and_then(|l| l.log_level))
        })
        .unwrap_or_else(|| "info".to_string()); // Default

    if log_level.contains('=') {
        for pair in log_level.split(',') {
            if pair.contains('=') {
                let mut kv_iter = pair.split('=').map(str::trim);
                if let (Some(module), Some(level)) = (kv_iter.next(), kv_iter.next()) {
                    log_builder.filter_module(module, get_log_level(level));
                }
            } else {
                log_builder.filter_level(get_log_level(pair));
            }
        }
    } else {
        // Set the log level based on the parsed value
        log_builder.filter_level(get_log_level(&log_level));
    }
    for module in LOG_ERROR_LEVEL_MOD {
        log_builder.filter_module(module, LevelFilter::Error);
    }
    log_builder.init();
    info!("Log Level {}", get_log_level(&log_level));
}
