macro_rules! json_field {
    ($settings:ident, $sys:ty; $($field:ident => $suffix:literal),* $(,)?) => {
        $(json_field!($settings, $sys, $field, $suffix);)*
    };
    ($settings:ident, $sys:ty, $field:ident, $suffix:literal) => {
        if let Some(s) = read_env::<$sys>($suffix)
            && let Some(v) = parse_json(&s)
        {
            $settings.$field = Some(v);
        }
    };
}
macro_rules! string_field {
    ($settings:ident, $sys:ty; $($field:ident => $suffix:literal),* $(,)?) => {
        $(string_field!($settings, $sys, $field, $suffix);)*
    };
    ($settings:ident, $sys:ty, $field:ident, $suffix:literal) => {
        if let Some(s) = read_env::<$sys>($suffix) {
            $settings.$field = Some(s);
        }
    };
}
macro_rules! enum_field {
    ($settings:ident, $sys:ty; $($field:ident => $suffix:literal, $ty:ty),* $(,)?) => {
        $(enum_field!($settings, $sys, $field, $suffix, $ty);)*
    };
    ($settings:ident, $sys:ty, $field:ident, $suffix:literal, $ty:ty) => {
        if let Some(s) = read_env::<$sys>($suffix)
            && let Some(v) = parse_json_or_string::<$ty>(&s)
        {
            $settings.$field = Some(v);
        }
    };
}
macro_rules! tri_array_field {
    ($settings:ident, $sys:ty; $($field:ident => $suffix:literal),* $(,)?) => {
        $(tri_array_field!($settings, $sys, $field, $suffix);)*
    };
    ($settings:ident, $sys:ty, $field:ident, $suffix:literal) => {
        if let Some(s) = read_env::<$sys>($suffix)
            && let Some(v) = parse_tri_array(&s)
        {
            $settings.$field = Some(v);
        }
    };
}
// Env vars cannot express the "explicit null clears" state that
// yaml supports (an empty value reads as unset — see `read_env`),
// so a present env var always lands as `Some(Some(s))`, never
// `Some(None)`. Same limitation as `tri_array_field!`.
macro_rules! tri_string_field {
    ($settings:ident, $sys:ty; $($field:ident => $suffix:literal),* $(,)?) => {
        $(tri_string_field!($settings, $sys, $field, $suffix);)*
    };
    ($settings:ident, $sys:ty, $field:ident, $suffix:literal) => {
        if let Some(s) = read_env::<$sys>($suffix) {
            $settings.$field = Some(Some(s));
        }
    };
}
