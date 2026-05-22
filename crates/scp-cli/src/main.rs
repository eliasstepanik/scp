use clap::Parser;

#[derive(Parser)]
#[command(name = "scp-cli")]
#[command(about = "SCP CLI (stub for Phase 0)", long_about = None)]
struct Args {}

fn main() {
    let _args = Args::parse();
    println!("scp-cli v0.1.0 (stub)");
}
