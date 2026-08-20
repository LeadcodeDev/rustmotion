fn main() {
    if let Err(e) = rustmotion::cli::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
