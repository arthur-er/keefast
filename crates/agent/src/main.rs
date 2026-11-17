//! `keefast-agent`: the Headless host and the commands its operator runs (ADR 0012).
//!
//! ```text
//! keefast-agent serve --vault <path | dropbox:/path> [--listen <address:port>]
//!                     [--tls-cert <path> --tls-key <path>] [--admin-socket <path>]
//!                     [--admin-group <group>] [--state-dir <dir>]
//!                     [--unattended-key-file <path>] [--unattended-password]
//! keefast-agent unlock [--key-file <path>] [--no-password] [--remember] [--admin-socket <path>]
//! keefast-agent lock [--admin-socket <path>]
//! keefast-agent status [--admin-socket <path>]
//! keefast-agent pairing create <name> [--reads <subgroup>]... [--can-create]
//!                       [--agent-group <path>] [--admin-socket <path>]
//! keefast-agent pairing approve <code> [--name <name>] [--reads <subgroup>]... [--can-create]
//!                        [--admin-socket <path>]
//! keefast-agent pairing list [--admin-socket <path>]
//! keefast-agent pairing revoke <name> [--admin-socket <path>]
//! keefast-agent pairing approver <name> [--admin-socket <path>]
//! keefast-agent pairing approvers [--admin-socket <path>]
//! keefast-agent pairing revoke-approver <name> [--admin-socket <path>]
//! keefast-agent log [--lines <n>] [--admin-socket <path>]
//! keefast-agent run --env NAME=<Secret reference>... -- <program> [args...]
//! ```
//!
//! docs/headless-host.md is the operator's guide.

use std::process::ExitCode;

const USAGE: &str = "\
keefast-agent: serve a keefast Vault to Agents, and look after it

usage:
  keefast-agent run --env NAME=<Secret reference>... -- <program> [args...]
      Runs <program> with the secrets the Secret references name in its environment, such as
      `run --env GH_TOKEN=GitHub/token#password -- gh auth status`, and redacts them from its
      output. It reaches keefast at $KEEFAST_AGENT_URL with the Agent pairing's token in
      $KEEFAST_AGENT_TOKEN. It exits with the program's exit code, or 125 if keefast refused
      or couldn't be reached.

  keefast-agent serve --vault <path | dropbox:/path> [--listen <address:port>]
                      [--tls-cert <path> --tls-key <path>] [--admin-socket <path>]
                      [--admin-group <group>] [--state-dir <dir>]
                      [--unattended-key-file <path>] [--unattended-password]
      Serves the Vault file as a Headless host. It starts locked: unlock it with
      `keefast-agent unlock`. Agents reach its MCP endpoint, /mcp, at --listen (127.0.0.1:7807
      by default) with an Agent pairing's token; an address other machines can reach needs
      TLS, from the PEM files --tls-cert and --tls-key. The admin socket is open only to this
      user, or also to --admin-group. With --unattended-key-file or --unattended-password it unlocks by itself
      at startup, from that Key file and the password `unlock --remember` kept in the OS secret
      store, and the Vault loses its protection at rest.

  keefast-agent unlock [--key-file <path>] [--no-password] [--remember] [--admin-socket <path>]
      Unlocks the Headless host with the Master key: the password, read from the terminal
      (or a line of input), and the Key file, if it has one. --no-password unlocks with the Key
      file alone. --remember keeps the password in the host's OS secret store for
      --unattended-password.

  keefast-agent lock [--admin-socket <path>]
      Locks the Headless host, saving its unsaved changes first.

  keefast-agent status [--admin-socket <path>]
      Says whether the Headless host is locked.

  keefast-agent pairing create <name> [--reads <subgroup>]... [--can-create]
                               [--agent-group <path>] [--admin-socket <path>]
      Pairs an Agent with the unlocked Vault: makes its subgroup <name> of the Agent group and
      prints its bearer token, once. It reads its own subgroup and each --reads subgroup.
      --can-create lets it create Entries in its own subgroup. If the Vault has no Agent group
      yet, --agent-group makes the Group at that path (such as Agents) the Agent group.

  keefast-agent pairing approve <code> [--name <name>] [--reads <subgroup>]... [--can-create]
                                [--admin-socket <path>]
      Approves an MCP client pairing by OAuth, whose consent page shows <code>: makes its
      Agent pairing, called <name> or else as the client calls itself, with its subgroup of
      the Agent group, reading each --reads subgroup too. The page then sends the browser back
      to the client, which gets its tokens. A client paired already is just let in again.

  keefast-agent pairing list [--admin-socket <path>]
      Lists the Vault's Agent pairings.

  keefast-agent pairing revoke <name> [--admin-socket <path>]
      Revokes an Agent pairing: its token stops working at once. Its subgroup stays.

  keefast-agent pairing approver <name> [--admin-socket <path>]
      Pairs a desktop keefast as an Approver called <name>, which shows the prompts for uses
      no Agent access decision answers, and prints its token, once. Add the host in keefast's
      Settings → Agent hosts with its address and this token. Works while the Vault is locked.

  keefast-agent pairing approvers [--admin-socket <path>]
      Lists the Approvers paired with the Headless host.

  keefast-agent pairing revoke-approver <name> [--admin-socket <path>]
      Unpairs an Approver: its token stops working.

  keefast-agent log [--lines <n>] [--admin-socket <path>]
      Shows the Headless host's audit log: what each Agent pairing listed and used, when, and
      whether it was allowed. The last 50 records, or <n>.

The admin socket is $KEEFAST_AGENT_ADMIN_SOCKET, or else $XDG_RUNTIME_DIR/keefast/agent-admin.sock.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("run") => run(&args[1..]),
        #[cfg(unix)]
        Some(command @ ("serve" | "unlock" | "lock" | "status" | "pairing" | "log")) => {
            match operator::run(command, &args[1..]) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("keefast-agent: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        #[cfg(not(unix))]
        Some("serve" | "unlock" | "lock" | "status" | "pairing" | "log") => {
            eprintln!("keefast-agent: the Headless host runs only on Linux and other unix systems");
            ExitCode::FAILURE
        }
        Some(other) => {
            eprintln!("keefast-agent: unknown command {other}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `keefast-agent run`: on Linux and Windows alike.
fn run(args: &[String]) -> ExitCode {
    use keefast_agent::run::{self, Connection, FAILED, RunArgs};

    let ran = RunArgs::parse(args).and_then(|args| {
        let connection = Connection::from_env()?;
        run::run(&args, &connection)
    });
    match ran {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("keefast-agent: {error}");
            ExitCode::from(FAILED)
        }
    }
}

#[cfg(unix)]
mod operator {
    use std::io::{self, BufRead, IsTerminal, Write};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use keefast_agent::admin::{
        self, AdminGroup, AdminReply, AdminRequest, AdminSocket, PairingCreate, PairingSummary,
    };
    use keefast_agent::host::{
        HeadlessHost, HostConfig, HostState, PairingChoice, UnattendedUnlock, describe,
        parse_location,
    };
    use keefast_agent::listen::{DEFAULT_LISTEN, Listen};
    use keefast_agent::oauth::OAuthServer;
    use keefast_agent::serve::McpEndpoint;
    use keefast_agent::{ApproverCredentials, ApproverHub, AuditLog, AuditOutcome, AuditRecord};
    use keefast_workspace::{
        Dropbox, DropboxConfig, FileVaultCache, LocalStorage, OsSecretStore, VaultLocation,
        Workspace,
    };
    use zeroize::Zeroizing;

    /// The command line after the command: flags, some with a value.
    struct Flags(Vec<String>);

    impl Flags {
        fn new(args: &[String], known: &[&str], with_value: &[&str]) -> Result<Self, String> {
            let mut iter = args.iter();
            while let Some(arg) = iter.next() {
                if with_value.contains(&arg.as_str()) {
                    iter.next().ok_or(format!("{arg} needs a value"))?;
                } else if !known.contains(&arg.as_str()) {
                    return Err(format!("unknown option {arg}: see keefast-agent --help"));
                }
            }
            Ok(Self(args.to_vec()))
        }

        /// Every value given for `name`, which may be repeated.
        fn values(&self, name: &str) -> Vec<String> {
            self.0
                .windows(2)
                .filter(|pair| pair[0] == name)
                .map(|pair| pair[1].clone())
                .collect()
        }

        fn value(&self, name: &str) -> Option<&str> {
            let at = self.0.iter().position(|arg| arg == name)?;
            self.0.get(at + 1).map(String::as_str)
        }

        fn has(&self, name: &str) -> bool {
            self.0.iter().any(|arg| arg == name)
        }

        fn admin_socket(&self) -> Result<PathBuf, String> {
            if let Some(path) = self.value("--admin-socket") {
                return Ok(PathBuf::from(path));
            }
            if let Some(path) = std::env::var_os("KEEFAST_AGENT_ADMIN_SOCKET") {
                return Ok(PathBuf::from(path));
            }
            // Never a shared directory such as /tmp (ADR 0004 update 2026-10-28).
            let dir = dirs::runtime_dir().ok_or(
                "there is no runtime directory (XDG_RUNTIME_DIR): name the admin socket \
                 with --admin-socket",
            )?;
            Ok(dir.join("keefast").join("agent-admin.sock"))
        }
    }

    pub fn run(command: &str, args: &[String]) -> Result<(), String> {
        match command {
            "serve" => serve(&Flags::new(
                args,
                &["--unattended-password"],
                &[
                    "--vault",
                    "--listen",
                    "--tls-cert",
                    "--tls-key",
                    "--admin-socket",
                    "--admin-group",
                    "--state-dir",
                    "--unattended-key-file",
                ],
            )?),
            "unlock" => unlock(&Flags::new(
                args,
                &["--no-password", "--remember"],
                &["--key-file", "--admin-socket"],
            )?),
            "pairing" => pairing(args),
            "log" => {
                let flags = Flags::new(args, &[], &["--lines", "--admin-socket"])?;
                let lines = match flags.value("--lines") {
                    Some(lines) => lines
                        .parse()
                        .map_err(|_| format!("--lines {lines}: give a number"))?,
                    None => 50,
                };
                show(ask(&flags, &AdminRequest::Log { lines })?, "")
            }
            "lock" => {
                let flags = Flags::new(args, &[], &["--admin-socket"])?;
                show(ask(&flags, &AdminRequest::Lock)?, "the Vault is locked")
            }
            _ => {
                let flags = Flags::new(args, &[], &["--admin-socket"])?;
                show(ask(&flags, &AdminRequest::Status)?, "")
            }
        }
    }

    fn serve(flags: &Flags) -> Result<(), String> {
        let vault = flags.value("--vault").ok_or("serve needs --vault")?;
        let location = parse_location(vault);
        // Checked before anything else, so a host that would hand out tokens in the clear
        // never starts.
        let listen = Listen::new(
            flags.value("--listen").unwrap_or(DEFAULT_LISTEN),
            flags.value("--tls-cert").map(PathBuf::from),
            flags.value("--tls-key").map(PathBuf::from),
        )
        .map_err(|error| error.to_string())?;
        let state_dir = match flags.value("--state-dir") {
            Some(dir) => PathBuf::from(dir),
            None => dirs::state_dir()
                .or_else(dirs::data_local_dir)
                .ok_or("there is no state directory: name one with --state-dir")?
                .join("keefast-agent"),
        };
        let workspace = workspace(&location, &state_dir)?;
        let unattended = (flags.has("--unattended-password")
            || flags.value("--unattended-key-file").is_some())
        .then(|| UnattendedUnlock {
            key_file: flags.value("--unattended-key-file").map(PathBuf::from),
            password: flags.has("--unattended-password"),
        });
        let group = flags
            .value("--admin-group")
            .map(AdminGroup::named)
            .transpose()
            .map_err(|error| format!("--admin-group: {error}"))?;
        let socket_path = flags.admin_socket()?;
        // Bound first, so a second host for the same socket stops before it unlocks anything.
        let socket = AdminSocket::bind(&socket_path, group)
            .map_err(|error| format!("can't listen at {}: {error}", socket_path.display()))?;
        let approver_credentials = ApproverCredentials::open(state_dir.join("approvers.json"))
            .map_err(|error| error.to_string())?;
        let oauth =
            OAuthServer::open(state_dir.join("oauth.json")).map_err(|error| error.to_string())?;
        let (host, warnings) = HeadlessHost::start(
            workspace,
            HostConfig {
                location: location.clone(),
                unattended,
                secret_store: Arc::new(OsSecretStore),
                audit_log: Arc::new(AuditLog::new(state_dir.join("audit.log"))),
                approvers: ApproverHub::new(),
                approver_credentials,
                oauth,
            },
        );
        for warning in warnings {
            eprintln!("keefast-agent: warning: {warning}");
        }
        let state = host.status().state;
        // The MCP endpoint runs on Tokio; the rest of the host on plain threads, as the
        // workspace is synchronous.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|error| format!("can't start the MCP endpoint: {error}"))?;
        let endpoint = runtime
            .block_on(McpEndpoint::start(Arc::clone(&host), &listen))
            .map_err(|error| error.to_string())?;
        socket.serve(Arc::clone(&host));
        eprintln!(
            "keefast-agent: serving {}, {}; admin socket {}; Agents connect to {}",
            describe(&location),
            match state {
                HostState::Unlocked => "unlocked",
                _ => "locked until `keefast-agent unlock`",
            },
            socket_path.display(),
            endpoint.url(),
        );
        runtime.block_on(std::future::pending::<()>());
        Ok(())
    }

    fn pairing(args: &[String]) -> Result<(), String> {
        let (action, rest) = args.split_first().ok_or(
            "pairing needs create, approve, list, revoke, approver, approvers or revoke-approver: see \
                 keefast-agent --help",
        )?;
        match action.as_str() {
            "create" => {
                let (name, rest) = rest
                    .split_first()
                    .filter(|(name, _)| !name.starts_with("--"))
                    .ok_or("pairing create needs the pairing's name")?;
                let flags = Flags::new(
                    rest,
                    &["--can-create"],
                    &["--reads", "--agent-group", "--admin-socket"],
                )?;
                let request = AdminRequest::PairingCreate(PairingCreate {
                    name: name.clone(),
                    reads: flags.values("--reads"),
                    can_create: flags.has("--can-create"),
                    agent_group: flags.value("--agent-group").map(str::to_owned),
                });
                match ask(&flags, &request)? {
                    AdminReply::PairingCreated { pairing, token } => {
                        eprintln!(
                            "Paired {}. Its token is below; it isn't shown again. Give it to \
                             the Agent as `Authorization: Bearer <token>`.",
                            describe_pairing(&pairing)
                        );
                        println!("{}", token.0.as_str());
                        Ok(())
                    }
                    other => show(other, ""),
                }
            }
            "approve" => {
                let (code, rest) = rest
                    .split_first()
                    .filter(|(code, _)| !code.starts_with("--"))
                    .ok_or("pairing approve needs the code the consent page shows")?;
                let flags = Flags::new(
                    rest,
                    &["--can-create"],
                    &["--name", "--reads", "--admin-socket"],
                )?;
                let request = AdminRequest::PairingApprove {
                    code: code.clone(),
                    pairing: PairingChoice {
                        name: flags.value("--name").unwrap_or_default().to_owned(),
                        reads: flags.values("--reads"),
                        can_create: flags.has("--can-create"),
                    },
                };
                show(ask(&flags, &request)?, "")
            }
            "list" => {
                let flags = Flags::new(rest, &[], &["--admin-socket"])?;
                show(ask(&flags, &AdminRequest::PairingList)?, "")
            }
            "revoke" => {
                let (name, rest) = rest
                    .split_first()
                    .filter(|(name, _)| !name.starts_with("--"))
                    .ok_or("pairing revoke needs the pairing's name")?;
                let flags = Flags::new(rest, &[], &["--admin-socket"])?;
                let request = AdminRequest::PairingRevoke { name: name.clone() };
                show(ask(&flags, &request)?, &format!("revoked {name}"))
            }
            "approver" => {
                let (name, rest) = rest
                    .split_first()
                    .filter(|(name, _)| !name.starts_with("--"))
                    .ok_or("pairing approver needs the Approver's name, such as laptop")?;
                let flags = Flags::new(rest, &[], &["--admin-socket"])?;
                match ask(&flags, &AdminRequest::ApproverPair { name: name.clone() })? {
                    AdminReply::ApproverPaired { approver, token } => {
                        eprintln!(
                            "Paired the Approver {}. Its token is below; it isn't shown again. In \
                             keefast on that desktop, add this host under Settings → Agent hosts, \
                             with the address Agents use and this token.",
                            approver.name
                        );
                        println!("{}", token.0.as_str());
                        Ok(())
                    }
                    other => show(other, ""),
                }
            }
            "approvers" => {
                let flags = Flags::new(rest, &[], &["--admin-socket"])?;
                show(ask(&flags, &AdminRequest::ApproverList)?, "")
            }
            "revoke-approver" => {
                let (name, rest) = rest
                    .split_first()
                    .filter(|(name, _)| !name.starts_with("--"))
                    .ok_or("pairing revoke-approver needs the Approver's name")?;
                let flags = Flags::new(rest, &[], &["--admin-socket"])?;
                let request = AdminRequest::ApproverRevoke { name: name.clone() };
                show(
                    ask(&flags, &request)?,
                    &format!("unpaired the Approver {name}"),
                )
            }
            other => Err(format!(
                "unknown pairing command {other}: it is create, approve, list, revoke, \
                 approver, approvers or revoke-approver"
            )),
        }
    }

    fn describe_pairing(pairing: &PairingSummary) -> String {
        let mut text = pairing.name.clone();
        if !pairing.reads.is_empty() {
            text += &format!(", which also reads {}", pairing.reads.join(", "));
        }
        if pairing.can_create {
            text += ", and may create Entries";
        }
        text
    }

    /// The Workspace a Headless host opens its Vault file through: local files, and Dropbox
    /// when the Vault file is there, with its encrypted copy cached in the state directory.
    fn workspace(location: &VaultLocation, state_dir: &Path) -> Result<Workspace, String> {
        let workspace = Workspace::new(Arc::new(LocalStorage));
        let VaultLocation::Dropbox(_) = location else {
            return Ok(workspace);
        };
        let config = dropbox_config()
            .ok_or("a Dropbox Vault file needs a Dropbox app key in KEEFAST_DROPBOX_APP_KEY")?;
        Ok(workspace
            .with_dropbox(Arc::new(Dropbox::new(config, Arc::new(OsSecretStore))))
            .with_vault_cache(Arc::new(FileVaultCache::new(state_dir.join("cache")))))
    }

    /// The Dropbox app keefast connects as, as the desktop app finds it: the environment, or
    /// the key it was built with. The `KEEFAST_DROPBOX_*_URL` variables point at another
    /// Dropbox, such as the fake one.
    fn dropbox_config() -> Option<DropboxConfig> {
        let env = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let app_key = env("KEEFAST_DROPBOX_APP_KEY")
            .or_else(|| option_env!("KEEFAST_DROPBOX_APP_KEY").map(str::to_owned))
            .filter(|key| !key.is_empty())?;
        let mut config = DropboxConfig::new(app_key);
        if let Some(url) = env("KEEFAST_DROPBOX_API_URL") {
            config.api_url = url;
        }
        if let Some(url) = env("KEEFAST_DROPBOX_CONTENT_URL") {
            config.content_url = url;
        }
        Some(config)
    }

    fn unlock(flags: &Flags) -> Result<(), String> {
        let key_file = flags
            .value("--key-file")
            .map(|path| {
                std::fs::read(path)
                    .map(Zeroizing::new)
                    .map_err(|error| format!("can't read the Key file {path}: {error}"))
            })
            .transpose()?;
        let password = if flags.has("--no-password") {
            if key_file.is_none() {
                return Err("--no-password needs --key-file".to_owned());
            }
            Zeroizing::new(String::new())
        } else {
            read_password("Master password: ")
                .map_err(|error| format!("can't read the password: {error}"))?
        };
        let request = AdminRequest::unlock(
            &password,
            key_file.as_deref().map(Vec::as_slice),
            flags.has("--remember"),
        );
        show(ask(flags, &request)?, "the Vault is unlocked")
    }

    fn ask(flags: &Flags, request: &AdminRequest) -> Result<AdminReply, String> {
        let socket = flags.admin_socket()?;
        admin::send(&socket, request).map_err(|error| {
            format!(
                "can't reach the Headless host at {}: {error}. Is `keefast-agent serve` running, \
                 and may you open its admin socket?",
                socket.display()
            )
        })
    }

    /// Prints the host's status, after `done` if it is one, or fails with its error.
    fn show(reply: AdminReply, done: &str) -> Result<(), String> {
        match reply {
            AdminReply::Status(status) => {
                if !done.is_empty() {
                    println!("{done}");
                }
                let state = match status.state {
                    HostState::Locked => "locked",
                    HostState::Unlocked => "unlocked",
                    HostState::Locking => {
                        "locking: Agents are shut out, but unsaved changes couldn't be saved yet"
                    }
                };
                println!("Vault: {}", status.vault);
                println!("State: {state}");
                println!(
                    "Unattended unlock: {}",
                    if status.unattended { "on" } else { "off" }
                );
                Ok(())
            }
            AdminReply::Pairings {
                agent_group,
                pairings,
            } => {
                if !done.is_empty() {
                    println!("{done}");
                }
                match agent_group {
                    Some(group) => println!("Agent group: {group}"),
                    None => println!("Agent group: none yet"),
                }
                if pairings.is_empty() {
                    println!("No Agent is paired.");
                }
                for pairing in &pairings {
                    println!("{}", describe_pairing(pairing));
                }
                Ok(())
            }
            AdminReply::Approvers { approvers } => {
                if !done.is_empty() {
                    println!("{done}");
                }
                if approvers.is_empty() {
                    println!("No Approver is paired.");
                }
                for approver in &approvers {
                    let since = keefast_core::Timestamp::from_unix_seconds(approver.created);
                    println!("{}, paired {}", approver.name, since.to_iso_8601());
                }
                Ok(())
            }
            AdminReply::ApproverPaired { approver, .. } => {
                println!("paired the Approver {}", approver.name);
                Ok(())
            }
            AdminReply::ConsentApproved {
                client,
                pairing,
                again,
            } => {
                if again {
                    println!("let {client} in again, as the Agent pairing {pairing}");
                } else {
                    println!("approved {client} as the Agent pairing {pairing}");
                }
                Ok(())
            }
            AdminReply::PairingCreated { pairing, .. } => {
                println!("paired {}", describe_pairing(&pairing));
                Ok(())
            }
            AdminReply::Log { records } => {
                if records.is_empty() {
                    println!("No Agent has used this Vault yet.");
                }
                for record in &records {
                    println!("{}", describe_record(record));
                }
                Ok(())
            }
            AdminReply::Error { message, .. } => Err(message),
        }
    }

    /// One line of the audit log: when, who, what, with what, and the outcome.
    fn describe_record(record: &AuditRecord) -> String {
        let time = keefast_core::Timestamp::from_unix_seconds(record.time).to_iso_8601();
        let mut line = format!("{time}  {}  {}", record.pairing, record.action);
        if let Some(entry) = &record.entry {
            line += &format!(" {} ({})", entry.title, entry.uuid);
        }
        if let Some(program) = &record.program {
            line += &format!(" with {program}");
        }
        line += match record.outcome {
            AuditOutcome::Allowed => "  allowed",
            AuditOutcome::Denied => "  denied",
        };
        if let Some(reason) = &record.reason {
            line += &format!(": {reason}");
        }
        line
    }

    /// Reads a password from the terminal without showing it, or else a line of input.
    fn read_password(prompt: &str) -> io::Result<Zeroizing<String>> {
        let stdin = io::stdin();
        let terminal = stdin.is_terminal();
        let _echo_off = if terminal {
            eprint!("{prompt}");
            io::stderr().flush()?;
            Some(EchoOff::new()?)
        } else {
            None
        };
        let mut line = Zeroizing::new(String::new());
        stdin.lock().read_line(&mut line)?;
        if terminal {
            eprintln!();
        }
        let password = line.trim_end_matches(['\n', '\r']);
        Ok(Zeroizing::new(password.to_owned()))
    }

    /// The terminal not echoing what is typed, until dropped.
    struct EchoOff(libc::termios);

    impl EchoOff {
        fn new() -> io::Result<Self> {
            // SAFETY: termios is plain data, filled in by tcgetattr before it is used.
            let mut original: libc::termios = unsafe { std::mem::zeroed() };
            // SAFETY: stdin's descriptor, and a valid pointer.
            if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut quiet = original;
            quiet.c_lflag &= !libc::ECHO;
            // SAFETY: as above.
            if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &quiet) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(original))
        }
    }

    impl Drop for EchoOff {
        fn drop(&mut self) {
            // SAFETY: restores what tcgetattr read.
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0) };
        }
    }
}
