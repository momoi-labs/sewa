use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

mod native;
mod opamp;
mod outbox;
mod plan;
mod state;
mod worker;

use opamp::{AGENT_VERSION, Host, POLL_INTERVAL};
use plan::Planner;
use state::State;
use worker::Worker;

/// The node side of sewa: reports this machine to the controller.
#[derive(Debug, Parser)]
#[command(name = "sewa-agent", version = AGENT_VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Trade an install token for a credential and store it.
    Enroll(EnrollArgs),
    /// Report this machine to the controller over OpAMP every 30 seconds,
    /// plan the configs it sends, and apply the current one when a pinned
    /// approver key signed its plan.
    Run(RunArgs),
    /// Pin the approver keys, read as JSON from stdin, replacing the keys
    /// pinned before. The install script runs this; with no key pinned the
    /// agent applies nothing.
    Pin(StateArgs),
}

#[derive(Debug, Args)]
struct StateArgs {
    /// Where the agent keeps its credential. Default: /var/lib/sewa on
    /// Linux, ~/Library/Application Support/sewa on macOS.
    #[arg(long, env = "SEWA_STATE_DIR")]
    state_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct EnrollArgs {
    /// The controller's address, such as http://controller.example:8080.
    #[arg(long)]
    server: String,
    /// The single-use install token from the controller.
    #[arg(long)]
    token: String,
    #[command(flatten)]
    state: StateArgs,
}

#[derive(Debug, Args)]
struct RunArgs {
    #[command(flatten)]
    state: StateArgs,
    /// The mise binary that plans the configs.
    #[arg(long, env = "SEWA_MISE", default_value = "mise")]
    mise: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Enroll(args) => enroll(args),
        Command::Run(args) => run(args),
        Command::Pin(args) => pin(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sewa-agent: {error}");
            ExitCode::FAILURE
        }
    }
}

fn state(args: StateArgs) -> Result<State, String> {
    match args.state_dir {
        Some(dir) => Ok(State::new(dir)),
        None => state::default_dir()
            .map(State::new)
            .map_err(|error| format!("cannot find the state directory: {error}")),
    }
}

/// The JSON string of `text`, for the one field of the enroll body.
fn json_string(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            character if (character as u32) < 0x20 => {
                quoted.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn enroll(args: EnrollArgs) -> Result<(), String> {
    let state = state(args.state)?;
    let server = args.server.trim().trim_end_matches('/').to_owned();
    let body = format!("{{\"token\":{}}}", json_string(args.token.trim()));
    let mut response = opamp::http_agent()
        .post(format!("{server}/v1/enroll"))
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(|error| format!("cannot reach {server}: {error}"))?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .with_config()
        .limit(4096)
        .read_to_string()
        .map_err(|error| format!("cannot read the answer: {error}"))?;
    match status {
        200 => {}
        401 => return Err("the token is unknown, used, or expired".to_owned()),
        403 => return Err("the controller does not see this machine on the tailnet".to_owned()),
        status => return Err(format!("the controller answered {status}: {}", text.trim())),
    }
    let credential = text.trim();
    if credential.is_empty() || !credential.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("the controller sent no credential".to_owned());
    }
    state
        .store_enrollment(&server, credential)
        .map_err(|error| {
            format!(
                "cannot store the credential in {}: {error}",
                state.dir.display()
            )
        })?;
    eprintln!("sewa-agent: enrolled with {server}");
    Ok(())
}

fn pin(args: StateArgs) -> Result<(), String> {
    let state = state(args)?;
    let mut input = String::new();
    std::io::stdin()
        .take(1024 * 1024)
        .read_to_string(&mut input)
        .map_err(|error| format!("cannot read the keys: {error}"))?;
    let pinned: sewa_approval::Pinned =
        serde_json::from_str(&input).map_err(|error| format!("cannot read the keys: {error}"))?;
    for key in &pinned.keys {
        sewa_approval::public_key(&key.public_key)
            .map_err(|_| format!("the key of {} does not decode", key.login))?;
    }
    state
        .store_approvers(&pinned)
        .map_err(|error| format!("cannot store the keys in {}: {error}", state.dir.display()))?;
    if pinned.keys.is_empty() {
        eprintln!(
            "sewa-agent: no approver key is pinned; this machine applies nothing until \
             a passkey is registered and the install runs again"
        );
    } else {
        eprintln!(
            "sewa-agent: pinned {} approver key(s) for {}",
            pinned.keys.len(),
            pinned.rp_id
        );
    }
    Ok(())
}

fn run(args: RunArgs) -> Result<(), String> {
    let state = state(args.state)?;
    let (server, credential) = state
        .enrollment()
        .map_err(|error| format!("not enrolled ({error}); run sewa-agent enroll first"))?;
    let uid = state
        .instance_uid()
        .map_err(|error| format!("cannot keep the instance UID: {error}"))?;
    let url = format!("{server}/v1/opamp");
    let agent = opamp::http_agent();
    let host = Host::this();
    eprintln!(
        "sewa-agent {AGENT_VERSION}: reporting to {server} as {}",
        host.user
    );
    if state.approvers().keys.is_empty() {
        eprintln!("sewa-agent: no approver key is pinned; this machine applies nothing");
    }
    let planner = Planner::new(&state.dir, args.mise);
    let mut worker = Worker::new(planner, state, uid);
    // Plans the last approved state on start, with or without the
    // controller.
    worker.boot();
    let mut sequence_num: u64 = 0;
    let mut failing = false;
    loop {
        let message = opamp::message(uid, sequence_num, &host);
        match worker.exchange(message, |message| {
            opamp::exchange(&agent, &url, &credential, message)
        }) {
            Ok(()) => {
                if failing {
                    eprintln!("sewa-agent: the controller answers again");
                }
                failing = false;
            }
            Err(error) => {
                // Once per outage: the agent keeps checking the host and
                // keeps its reports until the controller answers.
                if !failing {
                    eprintln!(
                        "sewa-agent: cannot report: {error}; reports wait in the outbox ({} now)",
                        worker.outbox.len()
                    );
                }
                failing = true;
            }
        }
        sequence_num = sequence_num.wrapping_add(1);
        worker.tick(std::time::Instant::now());
        // Buffered reports go out one per message, without waiting.
        if failing || worker.outbox.is_empty() {
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::json_string;

    #[test]
    fn quotes_json() {
        assert_eq!(json_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(json_string("a\nb"), r#""a\u000ab""#);
    }
}
