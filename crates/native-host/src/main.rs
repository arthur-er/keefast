//! `keefast-native-host`: Chromium starts it for the browser extension, with the extension's
//! origin as its argument, and talks to it over stdio.

use std::io;
use std::process::ExitCode;

use interprocess::local_socket::traits::Stream as _;
use keefast_native_host::{ipc, relay};

fn main() -> ExitCode {
    let path = ipc::socket_path();
    let result = relay::run(io::stdin().lock(), io::stdout(), || {
        Ok(ipc::connect(&path)?.split())
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("keefast-native-host: {error}");
            ExitCode::FAILURE
        }
    }
}
