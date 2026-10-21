use clap::Parser;

fn main() -> anyhow::Result<()> {
    temper_lean::cli::run(temper_lean::cli::LeanCli::parse().command)
}
