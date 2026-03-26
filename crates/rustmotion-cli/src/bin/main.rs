fn main() {
    if let Err(e) = rustmotion_cli::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
