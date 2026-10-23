//! `mnema`: command-line and MCP front for the app's agent socket.

mod client;
mod mcp;

use mnema_core::agent_wire::{ErrKind, Op, Request, VERSION, WireError};
use std::process::ExitCode;

const CLIENT: &str = "mnema-cli";

fn usage() -> WireError {
    WireError {
        kind: ErrKind::BadRequest,
        message: "usage: mnema search <query> [--k N] --json | passage <id> [--context N] --json | status --json | rescan [--wait] --json | mcp".into(),
    }
}

fn number(args: &mut Vec<String>, flag: &str) -> Result<Option<u32>, WireError> {
    let Some(i) = args.iter().position(|a| a == flag) else {
        return Ok(None);
    };
    if i + 1 >= args.len() {
        return Err(usage());
    }
    let raw = args.remove(i + 1);
    args.remove(i);
    raw.parse().map(Some).map_err(|_| usage())
}

fn flag(args: &mut Vec<String>, name: &str) -> bool {
    let n = args.len();
    args.retain(|a| a != name);
    args.len() != n
}

/// Everything after `--` is positional, so a query or id that looks like a flag
/// can still be passed (`mnema search -- --json`).
fn parse(mut args: Vec<String>) -> Result<Op, WireError> {
    let tail = match args.iter().position(|a| a == "--") {
        Some(i) => {
            let mut t = args.split_off(i);
            t.remove(0);
            t
        }
        None => Vec::new(),
    };
    flag(&mut args, "--json"); // output is always JSON; the flag is accepted for explicitness
    if args.is_empty() {
        return Err(usage());
    }
    let cmd = args.remove(0);
    let op = match cmd.as_str() {
        "search" => {
            let k = number(&mut args, "--k")?;
            args.extend(tail);
            Op::Search {
                query: args.pop().ok_or_else(usage)?,
                k,
            }
        }
        "passage" => {
            let context = number(&mut args, "--context")?;
            args.extend(tail);
            Op::Passage {
                id: args.pop().ok_or_else(usage)?,
                context,
            }
        }
        "status" => {
            args.extend(tail);
            Op::Status
        }
        "rescan" => {
            let wait = flag(&mut args, "--wait").then_some(true);
            args.extend(tail);
            Op::Rescan { wait }
        }
        _ => return Err(usage()),
    };
    if args.is_empty() {
        Ok(op)
    } else {
        Err(usage())
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("mcp") {
        return match mcp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("mnema mcp: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let result = parse(args).and_then(|op| {
        client::call(&Request {
            v: VERSION,
            client: Some(CLIENT.into()),
            op,
        })
    });
    match result {
        Ok(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!(
                "{}",
                serde_json::to_string(&e).expect("a WireError serialises")
            );
            ExitCode::FAILURE
        }
    }
}
