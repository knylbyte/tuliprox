#[macro_export]
macro_rules! from_impl {
    ($struct_name:ident) => {
        paste::paste! {
            impl From<[<$struct_name Dto>]> for $struct_name {
                fn from(dto: [<$struct_name Dto>]) -> Self {
                    $struct_name::from(&dto)
                }
            }
        }
    }
}

pub use from_impl;