//! `keefast-native-host`: Chromium starts it for the browser extension, with the extension's
//! origin as its argument, and talks to it over stdio.

use std::io;
use std::process::ExitCode;

use interprocess::local_socket::traits::Stream as _;
use keefast_native_host::{ipc, relay};

fn main() -> ExitCode {
    let result = relay::run(io::stdin().lock(), io::stdout(), || {
        // The browser hears only that keefast isn't running. Where it isn't simply that, the
        // browser's log says why.
        let path = ipc::socket_path().inspect_err(report)?;
        let stream = ipc::connect(&path).inspect_err(|error| {
            if error.kind() == io::ErrorKind::PermissionDenied {
                report(error);
            }
        })?;
        Ok(stream.split())
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&error);
            ExitCode::FAILURE
        }
    }
}

fn report(error: &io::Error) {
    eprintln!("keefast-native-host: {error}");
}
