//   AlgebraKing //
//
//   File:         [main.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Command-line dispatch: REPL, script, -e string and MCP server.']

mod ast;
mod builtins;
mod engine;
mod interp;
mod lexer;
mod mcp;
mod number;
mod parser;
mod printer;
mod repl;
mod simpl;

const USAGE: &str = "Usage: algebraking                 interactive session (REPL)
       algebraking file.mpl        run a script
       algebraking -e \"code\"       evaluate a string
       algebraking mcp             MCP server on stdio
Environment: ALGEBRAKING_PYTHON (default python3), ALGEBRAKING_TIMEOUT (seconds, default 60)";

fn real_main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => repl::run_repl(),
        [m] if m == "mcp" => mcp::run_server(),
        [e, code] if e == "-e" => repl::run_script(code),
        [h] if h == "-h" || h == "--help" => {
            println!("{}", USAGE);
            0
        }
        [v] if v == "-V" || v == "--version" => {
            println!("algebraking {}", env!("CARGO_PKG_VERSION"));
            0
        }
        [file] if !file.starts_with('-') => match std::fs::read_to_string(file) {
            Ok(src) => repl::run_script(&src),
            Err(e) => {
                eprintln!("Error, cannot read {}: {}", file, e);
                1
            }
        },
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

fn main() {
    // deep Maple recursion needs a large stack
    let worker = std::thread::Builder::new().stack_size(1 << 30).spawn(real_main).expect("cannot start main thread");
    let code = worker.join().unwrap_or(101);
    std::process::exit(code);
}
