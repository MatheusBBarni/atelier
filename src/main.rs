use multiagent::run_cli;

#[tokio::main]
async fn main() {
    if let Err(error) = run_cli().await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
