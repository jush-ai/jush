//! `jush-start.exe`: starts the rest of its own command line with no console
//! window, waits for it, and exits with its exit code.
//!
//! A Windows installation's scheduled task and its `Run` value start it, with
//! the PowerShell loop that runs the runner (`crates/runner/src/autostart.rs`),
//! and a request to run elevated starts it with the verb `runas`
//! (`crates/runner/src/elevation.rs`). Windows gives a program of the console
//! subsystem a console, which it hands to Windows Terminal where that is the
//! default terminal, and Windows Terminal shows a window however the program
//! asked to be hidden. This program is of the Windows subsystem, so it gets
//! none, and starts its command with `CREATE_NO_WINDOW`, which gives it a
//! console no one sees.
//!
//! It reads and decides nothing else, so an installation keeps it across
//! releases. It is compiled on its own by the runner's build script, with no
//! crate but `std`, and the runner carries it.

#![windows_subsystem = "windows"]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::os::windows::process::CommandExt as _;
use std::process::Command;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCommandLineW() -> *const u16;
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn main() {
    let line = command_line();
    // The command is passed on as written, past this program's own name:
    // a program reads its arguments from that text by its own rules, which
    // splitting and quoting them again could change.
    let command = skip_blanks(&line[token_end(&line)..]);
    let program_end = token_end(command);
    let program: Vec<u16> = command[..program_end]
        .iter()
        .copied()
        .filter(|&unit| unit != u16::from(b'"'))
        .collect();
    if program.is_empty() {
        std::process::exit(2);
    }
    let arguments = skip_blanks(&command[program_end..]);
    let mut started = Command::new(OsString::from_wide(&program));
    if !arguments.is_empty() {
        started.raw_arg(OsString::from_wide(arguments));
    }
    let code = match started.creation_flags(CREATE_NO_WINDOW).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => 1,
    };
    std::process::exit(code);
}

/// This process's command line, as Windows keeps it.
fn command_line() -> Vec<u16> {
    // SAFETY: GetCommandLineW returns a NUL-terminated string that lives as
    // long as the process.
    unsafe {
        let start = GetCommandLineW();
        let mut length = 0;
        while *start.add(length) != 0 {
            length += 1;
        }
        std::slice::from_raw_parts(start, length).to_vec()
    }
}

/// Where the program name that begins `text` ends: at its closing quote's
/// next character when it begins with a quote, else at the first blank, as
/// Windows reads a program name.
fn token_end(text: &[u16]) -> usize {
    let quote = u16::from(b'"');
    if text.first() == Some(&quote) {
        return text[1..]
            .iter()
            .position(|&unit| unit == quote)
            .map_or(text.len(), |close| close + 2);
    }
    text.iter().position(|&unit| is_blank(unit)).unwrap_or(text.len())
}

fn skip_blanks(text: &[u16]) -> &[u16] {
    let start = text.iter().position(|&unit| !is_blank(unit)).unwrap_or(text.len());
    &text[start..]
}

fn is_blank(unit: u16) -> bool {
    unit == u16::from(b' ') || unit == u16::from(b'\t')
}
