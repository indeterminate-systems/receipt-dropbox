use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = rdx_cli::main(&args, &mut std::io::stdout(), &mut std::io::stderr());
    ExitCode::from(code)
}
