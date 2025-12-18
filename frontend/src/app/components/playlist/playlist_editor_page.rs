use shared::error::TuliproxError;
use shared::info_err;
use std::fmt::Display;
use std::str::FromStr;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlaylistEditorPage {
    List,
    Create,
}

impl FromStr for PlaylistEditorPage {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        match s.to_lowercase().as_str() {
            "list" => Ok(PlaylistEditorPage::List),
            "create" => Ok(PlaylistEditorPage::Create),
            _ => Err(info_err!(format!("Unknown page type: {s}"))),
        }
    }
}

impl Display for PlaylistEditorPage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                Self::List => "list",
                Self::Create => "create",
            }
        )
    }
}
