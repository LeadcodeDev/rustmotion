fn main() {
    if let Err(e) = rustmotion_studio::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
