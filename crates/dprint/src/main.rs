use environment::{Environment, RealEnvironment};

mod configuration;
mod create_formatter;
mod environment;
mod run_cli;

fn main() {
    let environment = RealEnvironment::new();
    let args = std::env::args().collect();

    match run_cli::run_cli(&environment, args) {
        Err(err) => {
            environment.log_error(&err);
            std::process::exit(1);
        },
        _ => {},
    }
}
