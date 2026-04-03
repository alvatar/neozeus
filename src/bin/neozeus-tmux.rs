mod neozeus_tmux_support;

use std::env;

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if let Err(error) = neozeus_tmux_support::run(&args) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
