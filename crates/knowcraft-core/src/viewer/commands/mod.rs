//! Command table and dispatch, mirroring `main` and `command_arg_parse`.

pub mod annot;
pub mod attach;
pub mod basic;
pub mod info;
pub mod render;
pub mod search;
pub mod synctex;

use crate::viewer::protocol::arg::{parse_arg, ArgType, ArgValue};
use crate::viewer::protocol::encode::Response;
use crate::viewer::session::Session;

pub enum Flow {
    Continue,
    Quit,
}

pub type CmdResult = Result<Flow, String>;
pub type CmdFn = fn(&mut Session, &[ArgValue], &mut Response) -> CmdResult;

pub struct Command {
    pub name: &'static str,
    pub spec: &'static [ArgType],
    pub run: CmdFn,
}

use ArgType::*;

pub const COMMANDS: &[Command] = &[
    // Basic
    Command {
        name: "ping",
        spec: &[Str],
        run: basic::ping,
    },
    Command {
        name: "features",
        spec: &[],
        run: basic::features,
    },
    Command {
        name: "open",
        spec: &[NonEmptyString, Str],
        run: basic::open,
    },
    Command {
        name: "close",
        spec: &[NonEmptyString],
        run: basic::close,
    },
    Command {
        name: "quit",
        spec: &[],
        run: basic::quit,
    },
    Command {
        name: "getoptions",
        spec: &[Doc],
        run: basic::getoptions,
    },
    Command {
        name: "setoptions",
        spec: &[Doc, Rest],
        run: basic::setoptions,
    },
    // Searching
    Command {
        name: "search-string",
        spec: &[Doc, NatNum, NatNum, NonEmptyString, Bool],
        run: search::search_string,
    },
    Command {
        name: "search-regexp",
        spec: &[Doc, NatNum, NatNum, NonEmptyString, NatNum, NatNum],
        run: search::search_regexp,
    },
    Command {
        name: "regexp-flags",
        spec: &[],
        run: search::regexp_flags,
    },
    // General information
    Command {
        name: "metadata",
        spec: &[Doc],
        run: info::metadata,
    },
    Command {
        name: "outline",
        spec: &[Doc],
        run: info::outline,
    },
    Command {
        name: "number-of-pages",
        spec: &[Doc],
        run: info::number_of_pages,
    },
    Command {
        name: "pagelinks",
        spec: &[Doc, NatNum],
        run: info::pagelinks,
    },
    Command {
        name: "gettext",
        spec: &[Doc, NatNum, Edges, NatNum],
        run: info::gettext,
    },
    Command {
        name: "getselection",
        spec: &[Doc, NatNum, Edges, NatNum],
        run: info::getselection,
    },
    Command {
        name: "pagesize",
        spec: &[Doc, NatNum],
        run: info::pagesize,
    },
    Command {
        name: "boundingbox",
        spec: &[Doc, NatNum],
        run: info::boundingbox,
    },
    Command {
        name: "charlayout",
        spec: &[Doc, NatNum, EdgesOrPosition],
        run: info::charlayout,
    },
    Command {
        name: "pagelabels",
        spec: &[Doc],
        run: info::pagelabels,
    },
    // Annotations
    Command {
        name: "getannots",
        spec: &[Doc, NatNum, NatNum],
        run: annot::getannots,
    },
    Command {
        name: "getannot",
        spec: &[Doc, NonEmptyString],
        run: annot::getannot,
    },
    Command {
        name: "addannot",
        spec: &[Doc, NatNum, Str, NatNum, EdgesOrPosition, Rest],
        run: annot::addannot,
    },
    Command {
        name: "delannot",
        spec: &[Doc, NonEmptyString],
        run: annot::delannot,
    },
    Command {
        name: "editannot",
        spec: &[Doc, NonEmptyString, Rest],
        run: annot::editannot,
    },
    Command {
        name: "save",
        spec: &[Doc],
        run: annot::save,
    },
    // Attachments
    Command {
        name: "getattachment-from-annot",
        spec: &[Doc, NonEmptyString, Bool],
        run: attach::getattachment_from_annot,
    },
    Command {
        name: "getattachments",
        spec: &[Doc, Bool],
        run: attach::getattachments,
    },
    // SyncTeX
    Command {
        name: "synctex-forward-search",
        spec: &[Doc, NonEmptyString, NatNum, NatNum],
        run: synctex::forward_search,
    },
    Command {
        name: "synctex-backward-search",
        spec: &[Doc, NatNum, Edge, Edge],
        run: synctex::backward_search,
    },
    // Rendering
    Command {
        name: "renderpage",
        spec: &[Doc, NatNum, NatNum, Rest],
        run: render::renderpage,
    },
];

pub fn run(session: &mut Session, name: &str, raw: &[String]) -> Result<(Response, Flow), String> {
    let Some(command) = COMMANDS.iter().find(|c| c.name == name) else {
        return Err(format!("Unknown command: {name}"));
    };
    let mut out = Response::new();
    if command.spec.is_empty() {
        // Zero-argument commands ignore anything after the name, as the C main loop does.
        let flow = (command.run)(session, &[], &mut out)?;
        return Ok((out, flow));
    }

    let nargs = raw.len();
    let wanted = command.spec.len();
    let last_is_rest = command.spec.last() == Some(&Rest);
    let count_ok = if last_is_rest {
        nargs + 1 >= wanted
    } else {
        nargs == wanted
    };
    if !count_ok {
        return Err(format!(
            "Command `{}' expects {} argument(s), {} given",
            name, wanted, nargs
        ));
    }

    let mut parsed: Vec<ArgValue> = Vec::with_capacity(wanted);
    for (i, ty) in command.spec.iter().enumerate() {
        if *ty == Rest {
            parsed.push(ArgValue::Rest(raw[i..].to_vec()));
        } else {
            parsed.push(parse_arg(session, &raw[i], *ty)?);
        }
    }
    let flow = (command.run)(session, &parsed, &mut out)?;
    Ok((out, flow))
}
