use std::fmt::Display;

pub fn port_to_url(port: impl Display) -> String {
    format!("http://localhost:{port}/")
}
