use crate::{
    CONTROL_PROTOCOL_VERSION, ControlMethod, ControlRequest, UnixSocketAddress,
    resolve_control_socket,
};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;
use uuid::Uuid;

const CLI_MAX_ARGUMENT_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct CliInvocation {
    pub json: bool,
    pub socket: UnixSocketAddress,
    pub request: ControlRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliParseError {
    Help,
    Version,
    Usage(String),
}

impl fmt::Display for CliParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Help => formatter.write_str(cli_help()),
            Self::Version => formatter.write_str(concat!("kitmuxctl ", env!("CARGO_PKG_VERSION"))),
            Self::Usage(message) => write!(formatter, "{message}\n\n{}", cli_help()),
        }
    }
}

impl std::error::Error for CliParseError {}

pub fn parse_cli(
    arguments: impl IntoIterator<Item = String>,
    environment: &HashMap<String, String>,
) -> Result<CliInvocation, CliParseError> {
    let mut arguments = arguments.into_iter().collect::<Vec<_>>();
    let mut json = false;
    let mut socket = socket_from_environment(environment)?;
    while let Some(argument) = arguments.first().cloned() {
        match argument.as_str() {
            "--help" | "-h" => return Err(CliParseError::Help),
            "--version" | "-V" => return Err(CliParseError::Version),
            "--json" => {
                json = true;
                arguments.remove(0);
            }
            "--socket" => {
                arguments.remove(0);
                let value = arguments
                    .first()
                    .cloned()
                    .ok_or_else(|| usage("--socket requires an absolute path"))?;
                arguments.remove(0);
                socket = UnixSocketAddress::new(PathBuf::from(value))
                    .map_err(|error| usage(error.to_string()))?;
            }
            _ if argument.starts_with('-') => {
                return Err(usage(format!("unknown option {argument}")));
            }
            _ => break,
        }
    }
    let request = parse_command(&arguments)?;
    Ok(CliInvocation {
        json,
        socket,
        request,
    })
}

fn socket_from_environment(
    environment: &HashMap<String, String>,
) -> Result<UnixSocketAddress, CliParseError> {
    resolve_control_socket(environment, unsafe { libc::geteuid() })
        .map_err(|error| usage(error.to_string()))
}

fn parse_command(arguments: &[String]) -> Result<ControlRequest, CliParseError> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(CliParseError::Help);
    };
    let tail = &arguments[1..];
    match command {
        "ping" | "tree" | "identify" | "capabilities" => {
            if !tail.is_empty() {
                return Err(usage(format!("{command} takes no arguments")));
            }
            request(command, BTreeMap::new())
        }
        "events" => parse_events(tail),
        "ssh" => parse_ssh(tail),
        "request" => {
            let Some(method) = tail.first() else {
                return Err(usage("request requires a method"));
            };
            let mut params = BTreeMap::new();
            for argument in &tail[1..] {
                let Some((key, value)) = argument.split_once('=') else {
                    return Err(usage("request parameters must use key=value"));
                };
                insert_param(&mut params, key, value)?;
            }
            request(method, params)
        }
        "workspace" | "group" | "tab" => parse_hierarchy(command, tail),
        "pane" => parse_pane(tail),
        _ => Err(usage(format!("unknown command {command}"))),
    }
}

fn parse_hierarchy(noun: &str, arguments: &[String]) -> Result<ControlRequest, CliParseError> {
    let Some(action) = arguments.first().map(String::as_str) else {
        return Err(usage(format!(
            "{noun} requires new, select, rename, move, or close"
        )));
    };
    let method_noun = noun;
    match action {
        "new" if arguments.len() == 1 => request(&format!("{method_noun}.create"), BTreeMap::new()),
        "select" if arguments.len() == 2 => request(
            &format!("{method_noun}.select"),
            one_param("id", &arguments[1])?,
        ),
        "rename" if arguments.len() >= 3 => request(
            &format!("{method_noun}.rename"),
            one_param("id", &arguments[1])?
                .into_iter()
                .chain(one_param("name", &arguments[2..].join(" "))?)
                .collect(),
        ),
        "move" if arguments.len() == 3 => {
            let mut params = one_param("id", &arguments[1])?;
            insert_param(&mut params, "index", &arguments[2])?;
            request(&format!("{method_noun}.move"), params)
        }
        "close" if arguments.len() == 2 || arguments.len() == 3 => {
            let mut params = one_param("id", &arguments[1])?;
            if arguments.get(2).is_some_and(|value| value == "--force") {
                insert_param(&mut params, "force", "true")?;
            } else if arguments.len() == 3 {
                return Err(usage("close accepts only --force"));
            }
            request(&format!("{method_noun}.close"), params)
        }
        _ => Err(usage(format!(
            "usage: kitmuxctl {noun} new|select|rename|move|close ..."
        ))),
    }
}

fn parse_pane(arguments: &[String]) -> Result<ControlRequest, CliParseError> {
    let Some(action) = arguments.first().map(String::as_str) else {
        return Err(usage(
            "pane requires split, focus, rename, move, close, send, send-key, read-screen, or notify",
        ));
    };
    match action {
        "split" if arguments.len() == 2 && (arguments[1] == "right" || arguments[1] == "down") => {
            request("pane.split", one_param("axis", &arguments[1])?)
        }
        "focus" if arguments.len() == 2 => request("pane.focus", one_param("id", &arguments[1])?),
        "rename" if arguments.len() >= 3 => request(
            "pane.rename",
            one_param("id", &arguments[1])?
                .into_iter()
                .chain(one_param("name", &arguments[2..].join(" "))?)
                .collect(),
        ),
        "move" if arguments.len() == 3 => {
            let mut params = one_param("id", &arguments[1])?;
            insert_param(&mut params, "target", &arguments[2])?;
            request("pane.move", params)
        }
        "close" if arguments.len() == 2 || arguments.len() == 3 => {
            let mut params = one_param("id", &arguments[1])?;
            if arguments.get(2).is_some_and(|value| value == "--force") {
                insert_param(&mut params, "force", "true")?;
            } else if arguments.len() == 3 {
                return Err(usage("close accepts only --force"));
            }
            request("pane.close", params)
        }
        "send" if arguments.len() >= 3 => request(
            "pane.send",
            one_param("id", &arguments[1])?
                .into_iter()
                .chain(one_param("text", &arguments[2..].join(" "))?)
                .collect(),
        ),
        "send-key" if arguments.len() == 3 => request(
            "pane.send_key",
            one_param("id", &arguments[1])?
                .into_iter()
                .chain(one_param("key", &arguments[2])?)
                .collect(),
        ),
        "read-screen" if arguments.len() == 2 || arguments.len() == 3 => {
            let mut params = one_param("id", &arguments[1])?;
            if arguments.len() == 3 {
                insert_param(&mut params, "lines", &arguments[2])?;
            }
            request("pane.read_screen", params)
        }
        "notify" if arguments.len() >= 3 => request(
            "pane.notify",
            one_param("id", &arguments[1])?
                .into_iter()
                .chain(one_param("message", &arguments[2..].join(" "))?)
                .collect(),
        ),
        _ => Err(usage(
            "usage: kitmuxctl pane split|focus|rename|move|close|send|send-key|read-screen|notify ...",
        )),
    }
}

fn parse_events(arguments: &[String]) -> Result<ControlRequest, CliParseError> {
    let mut params = BTreeMap::new();
    let mut index = 0;
    while index < arguments.len() {
        let option = arguments[index].as_str();
        let Some(value) = arguments.get(index + 1) else {
            return Err(usage("events options require values"));
        };
        let key = match option {
            "--after" => "after",
            "--limit" => "limit",
            "--category" => "category",
            _ => return Err(usage("events accepts --after, --limit, and --category")),
        };
        insert_param(&mut params, key, value)?;
        index += 2;
    }
    request("event.list", params)
}

fn parse_ssh(arguments: &[String]) -> Result<ControlRequest, CliParseError> {
    if arguments.len() == 2 && arguments[0] == "profile" && arguments[1] == "list" {
        return request("ssh.profile.list", BTreeMap::new());
    }
    let Some(action) = arguments.first().map(String::as_str) else {
        return Err(usage(
            "usage: kitmuxctl ssh profile list|connect PROFILE_UUID|reconnect PANE_UUID",
        ));
    };
    if !matches!(action, "connect" | "reconnect") {
        return Err(usage(
            "usage: kitmuxctl ssh profile list|connect PROFILE_UUID|reconnect PANE_UUID",
        ));
    }
    if arguments.len() != 2 && arguments.len() != 4 {
        return Err(usage(format!(
            "ssh {action} requires an exact UUID and optional --approve FINGERPRINT"
        )));
    }
    let id = &arguments[1];
    if Uuid::parse_str(id).is_err() {
        return Err(usage(format!("ssh {action} requires an exact UUID")));
    }
    let mut params = BTreeMap::new();
    insert_param(
        &mut params,
        if action == "connect" {
            "profile"
        } else {
            "pane"
        },
        id,
    )?;
    if arguments.len() == 4 {
        if arguments[2] != "--approve" || !valid_fingerprint(&arguments[3]) {
            return Err(usage(
                "--approve requires a lowercase 64-character SHA-256 fingerprint",
            ));
        }
        insert_param(&mut params, "fingerprint", &arguments[3])?;
    }
    request(&format!("ssh.{action}"), params)
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn one_param(key: &str, value: &str) -> Result<BTreeMap<String, String>, CliParseError> {
    let mut params = BTreeMap::new();
    insert_param(&mut params, key, value)?;
    Ok(params)
}

fn insert_param(
    params: &mut BTreeMap<String, String>,
    key: &str,
    value: &str,
) -> Result<(), CliParseError> {
    if key.is_empty()
        || key.len() > 128
        || value.is_empty()
        || value.len() > CLI_MAX_ARGUMENT_BYTES
        || key.chars().any(char::is_control)
        || value.chars().any(char::is_control)
    {
        return Err(usage(
            "CLI identifiers and values must be non-empty, bounded, and control-free",
        ));
    }
    params.insert(key.to_owned(), value.to_owned());
    Ok(())
}

fn request(
    method: &str,
    params: BTreeMap<String, String>,
) -> Result<ControlRequest, CliParseError> {
    method
        .parse::<ControlMethod>()
        .map_err(|_| usage(format!("unsupported control method {method}")))?;
    Ok(ControlRequest {
        version: CONTROL_PROTOCOL_VERSION,
        id: format!("cli-{}", std::process::id()),
        method: method.to_owned(),
        params,
        context: None,
    })
}

fn usage(message: impl Into<String>) -> CliParseError {
    CliParseError::Usage(message.into())
}

#[must_use]
pub const fn cli_help() -> &'static str {
    "usage: kitmuxctl [--json] [--socket PATH] COMMAND\n\nCommands: ping, tree, identify, capabilities, events, ssh, request METHOD key=value..., workspace, group, tab, pane\nSSH: `ssh profile list`, `ssh connect PROFILE_UUID [--approve FINGERPRINT]`, `ssh reconnect PANE_UUID [--approve FINGERPRINT]`.\nUse `kitmuxctl pane send ID TEXT` to send text without invoking a shell."
}
