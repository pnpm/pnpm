#[cfg(unix)]
fn main() -> miette::Result<()> {
    pnpz::run()
}

#[cfg(not(unix))]
fn main() -> miette::Result<()> {
    Err(miette::miette!("pnpz supports Linux and macOS only"))
}
