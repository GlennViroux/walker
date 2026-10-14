use clap::Parser;
use std::path::Path;
use walker::utils::MemorySize;
use walker::walk::run;

#[derive(Parser)]
#[command(name = "Walker")]
#[command(about = "A fast, recursive file tree walker to gather analytics about file sizes")]
struct Args {
    #[arg(default_value = ".")]
    root: String,
    #[arg(long, default_value = "1")]
    num_threads: u8,
    #[arg(long, default_value = "14")]
    num_buckets: u16,
    #[arg(long, default_value = "16")]
    first_bucket_size_kb: u64,
}

fn main() {
    let args = Args::parse();
    let root_path = Path::new(&args.root);
    let initial_bucket_size = Some(MemorySize::from_kb(args.first_bucket_size_kb));
    run(
        root_path,
        args.num_threads,
        args.num_buckets,
        initial_bucket_size,
    )
    .unwrap_or_else(|err| {
        eprintln!("Something went wrong: {}", err);
        std::process::exit(1);
    });
}
