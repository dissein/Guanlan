//! Explicit live smoke check. Never runs as part of the offline test suite.
use guanlan_core::tools::Tools;
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut tools = Tools::new(std::env::current_dir()?);
    let output = tools
        .execute(
            "websearch",
            &json!({"query":"Rust programming language official", "numResults":3}),
            &CancellationToken::new(),
        )
        .await?;
    println!("{output}");
    Ok(())
}
