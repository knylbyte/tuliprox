#[macro_export]
macro_rules! check_input_credentials {
    ($this:ident, $input_type:expr, $definition:expr ) => {
        $this.url = $this.url.trim().to_string();
        if $this.url.is_empty() {
            return Err(info_err!("url for input is mandatory".to_string()));
        }
     $this.username = $crate::utils::get_trimmed_string(&$this.username);
     $this.password = $crate::utils::get_trimmed_string(&$this.password);

     match $input_type {
            InputType::M3u => {
                if $this.username.is_some() || $this.password.is_some() {
                    // TODO only for initial check
                    //return Err(info_err!("Input types of m3u should not use username or password".to_owned()));
                }
                let (username, password) = $crate::utils::get_credentials_from_url_str(&$this.url);
                $this.username = username;
                $this.password = password;
            }
            InputType::M3uBatch => {
                if $definition {
                  if $this.url.trim().is_empty() {
                        return Err(info_err!("for input type m3u-batch: url is mandatory".to_owned()));
                  }
                }
                if $this.username.is_some() || $this.password.is_some() {
                    // TODO only for initial check
                   // return Err(info_err!("Input types of m3u-batch should not define username or password".to_owned()));
                }
            }
            InputType::Xtream => {
                if $this.username.is_none() || $this.password.is_none() {
                    return Err(info_err!("for input type xtream: username and password are mandatory".to_owned()));
                }
            }
            InputType::XtreamBatch => {
                if $definition {
                  if $this.url.trim().is_empty() {
                        return Err(info_err!("for input type xtream-batch: url is mandatory".to_owned()));
                  }
                }
                if $this.username.is_some() || $this.password.is_some() {
                    return Err(info_err!("input type xtream-batch should not define username or password attribute ".to_owned()));
                }
            }
        }
    };
}

#[macro_export]
macro_rules! check_input_connections {
    ($this:ident, $input_type:expr) => {

     match $input_type {
            InputType::M3u |InputType::Xtream  => {
            }
            InputType::M3uBatch => {
                if $this.max_connections > 0 {
                    return Err(info_err!("input type m3u-batch should not define max_connections attribute ".to_owned()));
                }
                if $this.priority != 0 {
                    return Err(info_err!("input type m3u-batch should not define priority attribute ".to_owned()));
                }
            }
            InputType::XtreamBatch => {
                if $this.max_connections > 0 {
                    return Err(info_err!("input type xtream-batch should not define max_connections attribute ".to_owned()));
                }
                if $this.priority != 0 {
                    return Err(info_err!("input type xtream-batch should not define priority attribute ".to_owned()));
                }
            }
        }
    };
}

pub use check_input_credentials;
pub use check_input_connections;