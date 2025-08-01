#[derive(Debug, Clone, PartialEq)]
pub enum SearchRequest {
    Clear,
    Text(String),
    Regexp(String)
}