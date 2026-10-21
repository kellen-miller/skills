fn main() -> std::process::ExitCode {
    match skillctl::cli::run(std::env::args_os().collect(), &mut std::io::stdout().lock()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if let Some(error) = error.downcast_ref::<clap::Error>() {
                if let Err(print_error) = error.print() {
                    eprintln!("ERROR: could not print CLI error: {print_error}");
                }
                return std::process::ExitCode::from(error.exit_code() as u8);
            }

            eprintln!("ERROR: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
