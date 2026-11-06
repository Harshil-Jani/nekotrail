// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The `nekotrail` terminal command runs this binary as `nekotrail --cli <args>`: answer and
    // exit before any window or app state exists.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--cli") {
        std::process::exit(nekotrail_lib::cli::main(args[1..].to_vec()));
    }
    nekotrail_lib::run()
}
