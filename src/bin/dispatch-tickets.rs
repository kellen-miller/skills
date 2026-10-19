fn main() {
    if let Err(error) = skillctl::dispatch::run(std::env::args_os().collect()) {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}
