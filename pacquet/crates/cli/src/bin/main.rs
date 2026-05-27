#[tokio::main(flavor = "multi_thread")]
pub async fn main() -> miette::Result<()> {
    pacquet_cli::main().await
}
