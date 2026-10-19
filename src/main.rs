use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: cpl <song.cho>");
        return ExitCode::FAILURE;
    };
    if args.next().is_some() {
        eprintln!("usage: cpl <song.cho>");
        return ExitCode::FAILURE;
    }
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = cpl_song::parse(&source);
    println!("{}", result.song.first_meta("title").unwrap_or(&path));
    for section in &result.song.sections {
        println!("\n{}", section.label);
        for line in &section.lines {
            println!("{}", line.lyrics());
        }
    }
    for diagnostic in &result.diagnostics {
        eprintln!("{path}:{}: {}", diagnostic.line, diagnostic.message);
    }
    if result.diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
