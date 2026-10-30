//   AlgebraKing //
//
//   File:         [repl.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Interactive terminal session and script runner.']

use crate::engine::Engine;
use crate::interp::{Interp, Out};
use crate::parser::parse_program_partial;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::cell::RefCell;
use std::io::{IsTerminal, Write};
use std::rc::Rc;

fn new_interp() -> Interp {
    Interp::new(Rc::new(RefCell::new(Engine::new())))
}

/// Print the pending output of the interpreter; returns true if an error was printed.
fn flush(it: &mut Interp) -> bool {
    let outs: Vec<Out> = std::mem::take(&mut it.output);
    let pretty = it.prettyprint >= 1;
    let mut had_error = false;
    let mut stdout = std::io::stdout();
    for o in &outs {
        if matches!(o, Out::Error(_)) {
            had_error = true;
        }
        let text = it.render(o, pretty);
        match o {
            Out::Text(_) => {
                let _ = write!(stdout, "{}", text);
            }
            _ => {
                let _ = writeln!(stdout, "{}", text);
            }
        }
    }
    let _ = stdout.flush();
    had_error
}

/// Run a script (file or -e string). Stops at the first error with exit code 1.
pub fn run_script(src: &str) -> i32 {
    let mut it = new_interp();
    it.prettyprint = if std::io::stdout().is_terminal() { 1 } else { 0 };
    let (prog, perr) = parse_program_partial(src);
    for st in &prog {
        let quit = it.run_parsed(std::slice::from_ref(st), true);
        if flush(&mut it) {
            return 1;
        }
        if quit {
            return 0;
        }
    }
    match perr {
        Some(e) => {
            println!("Error, {}", e.msg);
            1
        }
        None => 0,
    }
}

fn history_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".algebraking_history"))
}

pub fn run_repl() -> i32 {
    let mut it = new_interp();
    it.prettyprint = 1;
    let mut rl = match DefaultEditor::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error, cannot start the line editor: {}", e);
            return 1;
        }
    };
    let hist = history_path();
    if let Some(h) = &hist {
        let _ = rl.load_history(h);
    }
    println!("AlgebraKing {} - symbolic math (SymPy engine). Type quit; to leave.", env!("CARGO_PKG_VERSION"));
    let mut buffer = String::new();
    loop {
        let prompt = if buffer.is_empty() { "> " } else { "  " };
        match rl.readline(prompt) {
            Ok(line) => {
                if buffer.is_empty() && line.trim().is_empty() {
                    continue;
                }
                buffer.push_str(&line);
                buffer.push('\n');
                let (prog, perr) = parse_program_partial(&buffer);
                if matches!(&perr, Some(e) if e.incomplete) {
                    continue;
                }
                let _ = rl.add_history_entry(buffer.trim_end());
                buffer.clear();
                let quit = it.run_parsed(&prog, false);
                flush(&mut it);
                if quit {
                    break;
                }
                if let Some(e) = perr {
                    println!("Error, {}", e.msg);
                }
            }
            Err(ReadlineError::Interrupted) => {
                buffer.clear();
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("Error, {}", e);
                break;
            }
        }
    }
    if let Some(h) = &hist {
        let _ = rl.save_history(h);
    }
    0
}
