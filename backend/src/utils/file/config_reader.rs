use crate::model::{ApiProxyConfig, SourcesConfig};
use crate::model::{Config};
use shared::error::{create_tuliprox_error,  info_err, to_io_error, TuliproxError, TuliproxErrorKind};
use crate::utils::{open_file, EnvResolvingReader};
use crate::utils::{file_reader};
use crate::utils::sys_utils::exit;
use shared::utils::CONSTANTS;
use chrono::Local;
use log::{error, info, warn};
use serde::Serialize;
use std::env;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{PathBuf};
use std::sync::Arc;
use shared::model::ConfigDto;
use crate::utils;

enum EitherReader<L, R> {
    Left(L),
    Right(R),
}

// `Read`-Trait für Either implementieren
impl<L: Read, R: Read> Read for EitherReader<L, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            EitherReader::Left(reader) => reader.read(buf),
            EitherReader::Right(reader) => reader.read(buf),
        }
    }
}

pub fn config_file_reader(file: File, resolve_env: bool) -> impl Read
{
    if resolve_env {
        EitherReader::Left(EnvResolvingReader::new(file_reader(file)))
    } else {
        EitherReader::Right(BufReader::new(file))
    }
}

pub fn read_api_proxy_config(cfg: &Config) -> Result<(), TuliproxError> {
    let api_proxy_config = read_api_proxy(cfg, true);
    match api_proxy_config {
        None => {
            warn!("cant read api_proxy_config file: {}", cfg.t_api_proxy_file_path.as_str());
            Ok(())
        }
        Some(config) => {
            cfg.set_api_proxy(Some(Arc::new(config)))?;
            Ok(())
        }
    }
}

pub fn read_sources(sources_file: &str, resolve_env: bool, include_computed: bool) -> Result<SourcesConfig, TuliproxError> {

    match open_file(&std::path::PathBuf::from(sources_file)) {
        Ok(file) => {
            let maybe_sources: Result<SourcesConfig, _> = serde_yaml::from_reader(config_file_reader(file, resolve_env));
            match maybe_sources {
                Ok(mut sources) => {
                    if let Err(err) = sources.prepare(include_computed) {
                        Err(info_err!(format!("Can't read the sources-config file: {sources_file}: {err}")))
                    } else {
                        Ok(sources)
                    }
                }
                Err(err) => Err(info_err!(format!("Can't read the sources-config file: {sources_file}: {err}")))
            }
        }
        Err(err) => Err(info_err!(format!("Can't read the sources-config file: {sources_file}: {err}")))
    }
}

pub fn read_config(config_path: &str, config_file: &str, sources_file: &str, api_proxy_file: &str, mappings_file: Option<String>, include_computed: bool) -> Result<Config, TuliproxError> {

    let resolve_env = true;
    let sources = read_sources(sources_file, resolve_env, include_computed)?;

    match open_file(&std::path::PathBuf::from(config_file)) {
        Ok(file) => {
            let maybe_config: Result<Config, _> = serde_yaml::from_reader(config_file_reader(file, resolve_env));
            match maybe_config {
                Ok(mut config) => {
                    config.sources = sources;
                    config.t_config_path = config_path.to_string();
                    config.t_config_file_path = config_file.to_string();
                    config.t_sources_file_path = sources_file.to_string();
                    config.t_api_proxy_file_path = api_proxy_file.to_string();
                    if let Err(err) = config.prepare(include_computed) { Err(err) } else {
                        if config.t_mapping_file_path.is_empty() {
                            config.t_mapping_file_path = resolve_env_var(&mappings_file.unwrap_or_else(|| utils::get_default_mappings_path(config_path)));
                        }
                        Ok(config)
                    }
                }
                Err(err) =>  Err(info_err!(format!("Can't read the config file: {config_file}: {err}")))
            }

        }
        Err(err) =>  Err(info_err!(format!("Can't read the config file: {config_file}: {err}")))
    }
}

pub fn read_api_proxy(config: &Config, resolve_env: bool) -> Option<ApiProxyConfig> {
    let api_proxy_file = config.t_api_proxy_file_path.as_str();
    open_file(&std::path::PathBuf::from(api_proxy_file)).map_or(None, |file| {
        let maybe_api_proxy: Result<ApiProxyConfig, _> = serde_yaml::from_reader(config_file_reader(file, resolve_env));
        match maybe_api_proxy {
            Ok(mut api_proxy) => {
                if let Err(err) = api_proxy.prepare() {
                    exit!("cant read api-proxy-config file: {err}");
                } else {
                    let mut errors = vec![];
                    api_proxy.migrate_api_user(config, &mut errors);
                    if !errors.is_empty() {
                        for error in errors {
                            error!("{error}");
                        }
                    }
                    Some(api_proxy)
                }
            }
            Err(err) => {
                error!("cant read api-proxy-config file: {err}");
                None
            }
        }
    })
}

fn write_config_file<T>(file_path: &str, backup_dir: &str, config: &T, default_name: &str) -> Result<(), TuliproxError>
where
    T: ?Sized + Serialize,
{
    let path = PathBuf::from(file_path);
    let filename = path.file_name().map_or(default_name.to_string(), |f| f.to_string_lossy().to_string());
    let backup_path = PathBuf::from(backup_dir).join(format!("{filename}_{}", Local::now().format("%Y%m%d_%H%M%S")));


    match std::fs::copy(&path, &backup_path) {
        Ok(_) => {}
        Err(err) => { error!("Could not backup file {}:{}", &backup_path.to_str().unwrap_or("?"), err) }
    }
    info!("Saving file to {}", &path.to_str().unwrap_or("?"));

    File::create(&path)
        .and_then(|f| serde_yaml::to_writer(f, &config).map_err(to_io_error))
        .map_err(|err| create_tuliprox_error!(TuliproxErrorKind::Info, "Could not write file {}: {}", &path.to_str().unwrap_or("?"), err))
}

pub fn save_api_proxy(file_path: &str, backup_dir: &str, config: &ApiProxyConfig) -> Result<(), TuliproxError> {
    write_config_file(file_path, backup_dir, config, "api-proxy.yml")
}

pub fn save_main_config(file_path: &str, backup_dir: &str, config: &ConfigDto) -> Result<(), TuliproxError> {
    write_config_file(file_path, backup_dir, config, "config.yml")
}

pub fn resolve_env_var(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    CONSTANTS.re_env_var.replace_all(value, |caps: &regex::Captures| {
        let var_name = &caps["var"];
        env::var(var_name).unwrap_or_else(|e| {
            error!("Could not resolve env var '{var_name}': {e}");
            format!("${{env:{var_name}}}")
        })
    }).to_string()
}

#[cfg(test)]
mod tests {
    use crate::utils::resolve_env_var;

    #[test]
    fn test_resolve() {
        let resolved = resolve_env_var("${env:HOME}");
        assert_eq!(resolved, std::env::var("HOME").unwrap());
    }
}