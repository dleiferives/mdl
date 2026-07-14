use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match mdl::run(env::args_os()) {
        Ok(output) => {
            if let Err(error) = write_output(io::stdout().lock(), &output) {
                eprintln!("mdl: failed to write standard output: {error}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

fn write_output(mut destination: impl Write, output: &str) -> io::Result<()> {
    destination.write_all(output.as_bytes())?;
    if !output.ends_with('\n') {
        destination.write_all(b"\n")?;
    }
    Ok(())
}
