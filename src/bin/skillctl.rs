fn main() -> std::process::ExitCode {
    match skillctl::cli::run(std::env::args_os().collect(), &mut std::io::stdout().lock()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if let Some(error) = error.downcast_ref::<clap::Error>() {
                let _ = error.print();
                return std::process::ExitCode::from(error.exit_code() as u8);
            }

            eprintln!("ERROR: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
