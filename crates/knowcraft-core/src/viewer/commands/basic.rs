//! `ping`, `features`, `open`, `close`, `quit`, `getoptions`, `setoptions`.

use crate::viewer::commands::{CmdResult, Flow};
use crate::viewer::options::NAMES;
use crate::viewer::protocol::arg::{parse_arg, ArgType, ArgValue};
use crate::viewer::protocol::encode::Response;
use crate::viewer::session::Session;

pub fn ping(_session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    out.ok_begin();
    out.line(Some(args[0].string()));
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn features(_session: &mut Session, _args: &[ArgValue], out: &mut Response) -> CmdResult {
    out.ok_begin();
    out.raw("case-sensitive-search:writable-annotations:markup-annotations\n");
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn open(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let filename = args[0].string();
    let password = args[1].string();
    let password = if password.is_empty() {
        None
    } else {
        Some(password)
    };
    session
        .open(filename, password)
        .map_err(|reason| format!("Error opening {filename}:{reason}"))?;
    out.ok();
    Ok(Flow::Continue)
}

pub fn close(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let was_open = session.close(args[0].string());
    out.ok_begin();
    out.raw(if was_open { "1\n" } else { "0\n" });
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn quit(session: &mut Session, _args: &[ArgValue], out: &mut Response) -> CmdResult {
    session.clear();
    out.ok();
    Ok(Flow::Quit)
}

pub fn getoptions(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let doc = session.require_mut(args[0].doc())?;
    out.ok_begin();
    for record in doc.options().records() {
        out.raw(&record);
        out.newline();
    }
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn setoptions(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let rest = args[1].rest();
    if !rest.len().is_multiple_of(2) {
        return Err("Even number of key/value pairs expected".to_string());
    }
    let mut updates: Vec<(usize, ArgValue)> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let name = match parse_arg(session, &rest[i], ArgType::NonEmptyString)? {
            ArgValue::Str(s) => s,
            _ => unreachable!(),
        };
        i += 1;
        let slot = if name == NAMES[0] {
            0
        } else if name == NAMES[1] {
            1
        } else if name == NAMES[2] {
            2
        } else if name == NAMES[3] {
            3
        } else {
            return Err(format!("Unknown option: {name}"));
        };
        let ty = match slot {
            0 => ArgType::NatNum,
            1 => ArgType::Bool,
            _ => ArgType::Color,
        };
        let value = parse_arg(session, &rest[i], ty)?;
        updates.push((slot, value));
        i += 1;
    }
    let doc = session.require_mut(&key)?;
    {
        let opts = doc.options_mut();
        for (slot, value) in updates {
            match (slot, value) {
                (0, ArgValue::NatNum(n)) => opts.usecolors = n,
                (1, ArgValue::Bool(b)) => opts.printed = b,
                (2, ArgValue::Color(c)) => opts.foreground = c,
                (3, ArgValue::Color(c)) => opts.background = c,
                _ => unreachable!(),
            }
        }
    }
    out.ok_begin();
    for record in doc.options().records() {
        out.raw(&record);
        out.newline();
    }
    out.ok_end();
    Ok(Flow::Continue)
}
