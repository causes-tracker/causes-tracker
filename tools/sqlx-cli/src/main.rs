// Entry point for the sqlx CLI tool.
// Delegates entirely to the sqlx-cli library; this file exists so that
// Bazel can build a hermetic rust_binary without a cargo install step.
use clap::Parser;
use console::style;
use sqlx_cli::Opt;

#[tokio::main]
async fn main() {
    sqlx_cli::maybe_apply_dotenv();
    // sqlx 0.9 no longer auto-registers Any drivers; install them so that
    // the CLI's Any-based connection handling (e.g. `prepare`) can connect.
    sqlx::any::install_default_drivers();
    let opt = Opt::parse();
    if let Err(error) = sqlx_cli::run(opt).await {
        println!("{} {}", style("error:").bold().red(), error);
        std::process::exit(1);
    }
}
