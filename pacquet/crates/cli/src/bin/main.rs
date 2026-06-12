pub fn main() -> miette::Result<()> {
    // Must run before the tokio runtime exists: it writes an
    // environment variable, which is only sound while the process is
    // single-threaded.
    pacquet_cli::configure_rayon_pool();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build the tokio runtime")
        .block_on(pacquet_cli::main())
}
