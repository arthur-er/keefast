//! The programs `keefast-agent run` starts, as Agent access decisions know them (ADR 0012).
//!
//! A decision names a program by its executable's name, wherever it is and whatever the
//! platform adds to it, so allowing `gh` allows `/usr/bin/gh` and `C:\Tools\gh.exe` alike.
//!
//! Shells and interpreters run whatever they are told, so allowing one for good would allow
//! anything: decisions for them are never remembered, and they ask every time. The list is
//! fixed and matched on the executable's name; it can't be complete, which is why the Agent's
//! command line is shown whole when it asks.

/// The name decisions know `program` by: its executable's file name, without a Windows
/// executable extension.
pub fn program_name(program: &str) -> String {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let lower = name.to_ascii_lowercase();
    for extension in [".exe", ".com", ".cmd", ".bat"] {
        if lower.ends_with(extension) && name.len() > extension.len() {
            return name[..name.len() - extension.len()].to_owned();
        }
    }
    name.to_owned()
}

/// Shells, interpreters, and programs that run another program they are given.
const NEVER_REMEMBERED: &[&str] = &[
    // Shells.
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ash",
    "ksh",
    "mksh",
    "csh",
    "tcsh",
    "nu",
    "elvish",
    "xonsh",
    "busybox",
    "pwsh",
    "powershell",
    "powershell_ise",
    "cmd",
    "wsl",
    // Interpreters and their package runners.
    "node",
    "nodejs",
    "deno",
    "bun",
    "bunx",
    "npx",
    "ruby",
    "irb",
    "perl",
    "php",
    "tclsh",
    "wish",
    "osascript",
    "wscript",
    "cscript",
    "rscript",
    "julia",
    "awk",
    "gawk",
    "mawk",
    "uv",
    "uvx",
    "pipx",
    // Programs that start another.
    "env",
    "xargs",
    "exec",
    "sudo",
    "doas",
    "su",
    "nohup",
    "nice",
    "timeout",
    "time",
    "watch",
    "find",
    "strace",
    "gdb",
    "start",
];

/// Prefixes of interpreters that come in versions: `python3.12`, `pypy3`, `lua5.4`.
const NEVER_REMEMBERED_PREFIXES: &[&str] = &["python", "pypy", "lua"];

/// Whether `program` is a shell or interpreter, whose decisions are never remembered.
pub fn is_shell_or_interpreter(program: &str) -> bool {
    let name = program_name(program).to_ascii_lowercase();
    NEVER_REMEMBERED.contains(&name.as_str())
        || NEVER_REMEMBERED_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}
