fn main() {
    if let Err(e) = rustmotion::studio::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
