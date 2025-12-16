#[macro_export]
macro_rules! add_str_property_if_exists {
    ($vec:expr, $prop:expr, $prop_name:expr) => {
        $vec.insert(String::from($prop_name), Value::String($prop.to_string()));
    }
}

#[macro_export]
macro_rules! add_rc_str_property_if_exists {
    ($vec:expr, $prop:expr, $prop_name:expr) => {
       $prop.as_ref().map(|v| $vec.insert(String::from($prop_name), Value::String(v.to_string())));
    }
}

#[macro_export]
macro_rules! add_opt_i64_property_if_exists {
    ($vec:expr, $prop:expr, $prop_name:expr) => {
       $prop.as_ref().map(|v| $vec.insert(String::from($prop_name), Value::Number(serde_json::value::Number::from(i64::from(*v)))));
    }
}

#[macro_export]
macro_rules! add_opt_f64_property_if_exists {
    ($vec:expr, $prop:expr, $prop_name:expr) => {
       $prop.as_ref().map(|v| $vec.insert(String::from($prop_name), Value::Number(serde_json::value::Number::from_f64(f64::from(*v)).unwrap_or_else(|| serde_json::Number::from(0)))));
    }
}

#[macro_export]
macro_rules! add_f64_property_if_exists {
    ($vec:expr, $prop:expr, $prop_name:expr) => {
       $vec.insert(String::from($prop_name), Value::Number(serde_json::value::Number::from_f64(f64::from($prop)).unwrap_or_else(|| serde_json::Number::from(0))));
    }
}

#[macro_export]
macro_rules! add_i64_property_if_exists {
    ($vec:expr, $prop:expr, $prop_name:expr) => {
       $vec.insert(String::from($prop_name), Value::Number(serde_json::value::Number::from(i64::from($prop))));
    }
}

#[macro_export]
macro_rules! add_to_doc_str_property_if_not_exists {
    ($document:expr, $prop_name:expr, $prop_value:expr) => {
          match $document.get($prop_name) {
            None => {
                $document.insert(String::from($prop_name), $prop_value);
            }
            Some(value) => { if Value::is_null(value) {
                $document.insert(String::from($prop_name), $prop_value);
            }}
          }
    }
}


pub use add_str_property_if_exists;
pub use add_rc_str_property_if_exists;
pub use add_opt_i64_property_if_exists;
pub use add_opt_f64_property_if_exists;
pub use add_f64_property_if_exists;
pub use add_i64_property_if_exists;
pub use add_to_doc_str_property_if_not_exists;
