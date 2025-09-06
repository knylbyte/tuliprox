use std::borrow::Cow;

pub trait Capitalize {
    fn capitalize(&self) -> String;
}

// Implement the Capitalize trait for &str
impl<T: AsRef<str>> Capitalize for T {
    fn capitalize(&self) -> String {
        let s = self.as_ref();
        let mut chars = s.chars();
        let first = chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>())
            .unwrap_or_default();
        let rest = chars.as_str().to_lowercase();
        first + &rest
    }
}

pub fn get_trimmed_string(value: &Option<String>) -> Option<String> {
    if let Some(v) = value {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub fn generate_random_string(length: usize) -> String {
    let mut rng = fastrand::Rng::new();
    let random_string: String = (0..length).map(|_| rng.alphanumeric()).collect();
    random_string
}

// compare 2 small vecs without HashSet
pub fn small_vecs_equal_unordered<T: PartialEq>(a: &[T], b: &[T]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    for item in a {
        if !b.iter().any(|x| x == item) {
            return false;
        }
    }
    true
}

pub fn get_non_empty_str<'a>(first: &'a str, second: &'a str, third: &'a str) -> &'a str {
    if !first.is_empty() {
        first
    } else if !second.is_empty() {
        second
    } else {
        third
    }
}

pub fn is_blank_optional_string(s: &Option<String>) -> bool {
    s.is_none() || s.as_ref().is_some_and(|s| s.trim().is_empty())
}

pub fn trim_slash(s: &str) -> Cow<'_, str> {
    let trimmed = s.trim_matches('/');
    if trimmed.len() == s.len() {
        Cow::Borrowed(s) // Keine Änderung → kein Clone
    } else {
        Cow::Owned(trimmed.to_string()) // Änderung → neue String
    }
}

pub fn trim_last_slash(s: &str) -> Cow<'_, str> {
    if s.ends_with('/') {
        if let Some(stripped) = s.strip_suffix('/') {
          return  Cow::Owned(stripped.to_string())
        }
    }
    Cow::Borrowed(s)
}

pub trait Substring {
    fn substring(&self, from: usize, to: usize) -> String;
}

impl Substring for String {
    fn substring(&self, from: usize, to: usize) -> String {
        self.chars().skip(from).take(to - from).collect()
    }
}


#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use crate::utils::Capitalize;
    use super::generate_random_string;

    #[test]
    fn test_generate_random_string() {
        let mut strings = HashSet::new();
        for _i in 0..100 {
            strings.insert(generate_random_string(5));
        }
        assert_eq!(strings.len(), 100);
    }

    #[test]
    fn test_capitalize() {
        assert_eq!("hELLO".capitalize(), "Hello");
    }

}