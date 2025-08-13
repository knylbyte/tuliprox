use std::fmt;
use std::str::FromStr;
use shared::error::{info_err, TuliproxError, TuliproxErrorKind};

const DASHBOARD: &'static str = "dashboard";
const STATS: &'static str = "stats";
const USERS: &'static str = "users";
const CONFIG: &'static str = "config";
const PLAYLIST_UPDATE: &'static str = "playlist_update";
const PLAYLIST_EDITOR: &'static str = "playlist_editor";
const PLAYLIST_EXPLORER: &'static str = "playlist_explorer";


#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewType {
    Dashboard,
    Stats,
    Users,
    Config,
    PlaylistUpdate,
    PlaylistEditor,
    PlaylistExplorer
}

impl FromStr for ViewType {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        match s.to_lowercase().as_str() {
            DASHBOARD => Ok(ViewType::Dashboard),
            STATS => Ok(ViewType::Stats),
            USERS => Ok(ViewType::Users),
            CONFIG => Ok(ViewType::Config),
            PLAYLIST_UPDATE => Ok(ViewType::PlaylistUpdate),
            PLAYLIST_EDITOR => Ok(ViewType::PlaylistEditor),
            PLAYLIST_EXPLORER => Ok(ViewType::PlaylistExplorer),
            _ => Err(info_err!(format!("Unknown view type: {s}"))),
        }
    }
}

impl fmt::Display for ViewType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ViewType::Dashboard => DASHBOARD,
            ViewType::Stats => STATS,
            ViewType::Users => USERS,
            ViewType::Config => CONFIG,
            ViewType::PlaylistUpdate => PLAYLIST_UPDATE,
            ViewType::PlaylistEditor => PLAYLIST_EDITOR,
            ViewType::PlaylistExplorer => PLAYLIST_EXPLORER,
        };
        write!(f, "{s}")
    }
}