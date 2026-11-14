use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

mod opamp;
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
    /// and plan the configs it sends. The agent never applies a plan.
    Run(RunArgs),
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
    let mut worker = Worker::new(Planner::new(&state.dir, args.mise));
    let mut sequence_num: u64 = 0;
    let mut failing = false;
    loop {
        let mut message = opamp::message(uid, sequence_num, &host);
        let report = worker.fill(&mut message);
        match opamp::exchange(&agent, &url, &credential, &message) {
            Ok(answer) => {
                if failing {
                    eprintln!("sewa-agent: the controller answers again");
                }
                failing = false;
                if let Some(remote) = answer.remote_config {
                    worker.receive(remote);
                }
            }
            Err(error) => {
                eprintln!("sewa-agent: cannot report: {error}");
                failing = true;
                if let Some(report) = report {
                    worker.queue.push_front(report);
                }
            }
        }
        sequence_num = sequence_num.wrapping_add(1);
        worker.tick(std::time::Instant::now());
        // Queued plans go out one per message, without waiting.
        if failing || worker.queue.is_empty() {
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
