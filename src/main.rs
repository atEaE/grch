use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    // 入力として受け取るファイルパス
    #[arg(short, long, num_args = 1.., required = true)]
    input: Vec<PathBuf>,
}

fn main() {
    let args = Args::parse();

    for path in &args.input {
        if path.is_file() {
            println!("{}", path.display())
        } else {
            eprintln!("skip: {} (file not found)", path.display())
        }
    }
}
