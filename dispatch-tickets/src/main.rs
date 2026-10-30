fn main() {
    if let Err(error) = dispatch_tickets::run(std::env::args_os().collect()) {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}
