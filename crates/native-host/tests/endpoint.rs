//! Where the app listens for the native host, and that the host relays only to a keefast of
//! the same user (ADR 0004).

use std::io;

use keefast_native_host::ipc::{self, Server};

#[cfg(unix)]
mod unix {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn the_socket_lives_in_the_runtime_directory() {
        let path = ipc::socket_path_in(Some(PathBuf::from("/run/user/1000"))).unwrap();

        assert_eq!(path, PathBuf::from("/run/user/1000/keefast/browser.sock"));
    }

    #[test]
    fn without_a_runtime_directory_there_is_no_socket() {
        let error = ipc::socket_path_in(None).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("runtime directory"), "{error}");
    }

    #[test]
    fn the_app_does_not_listen_in_a_directory_open_to_other_users() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, Permissions::from_mode(0o777)).unwrap();

        let refused = Server::start(shared.join("browser.sock"), |_| true);

        assert_eq!(
            refused.err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(!shared.join("browser.sock").exists());
    }

    #[test]
    fn the_host_connects_to_an_app_of_its_own_user() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keefast").join("browser.sock");
        let _server = Server::start(path.clone(), |_| true).unwrap();

        assert!(ipc::connect(&path).is_ok());
    }
}

#[cfg(windows)]
mod windows {
    use std::path::PathBuf;

    use super::*;

    fn pipe(test: &str) -> PathBuf {
        PathBuf::from(format!(
            r"\\.\pipe\keefast-test-{}-{test}",
            std::process::id()
        ))
    }

    #[test]
    fn the_pipe_is_named_after_the_user() {
        let user = std::env::var("USERNAME").unwrap_or_default();

        assert_eq!(
            ipc::socket_path().unwrap(),
            PathBuf::from(format!(r"\\.\pipe\keefast-browser-{user}"))
        );
    }

    // The pipe lets only this user connect, and the host checks the app serving it runs as
    // this user: both hold for an app of the user's own.
    #[test]
    fn the_host_connects_to_an_app_of_its_own_user() {
        let path = pipe("own-user");
        let _server = Server::start(path.clone(), |_| true).unwrap();

        assert!(ipc::connect(&path).is_ok());
    }

    #[test]
    fn a_second_app_cannot_take_over_the_pipe() {
        let path = pipe("take-over");
        let _first = Server::start(path.clone(), |_| true).unwrap();

        assert!(Server::start(path, |_| true).is_err());
    }
}

#[test]
fn nothing_listening_is_not_mistaken_for_another_user() {
    #[cfg(unix)]
    let (_dir, path) = {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("browser.sock");
        (dir, path)
    };
    #[cfg(windows)]
    let path = std::path::PathBuf::from(format!(
        r"\\.\pipe\keefast-test-{}-nothing",
        std::process::id()
    ));

    let error = ipc::connect(&path).unwrap_err();

    assert_ne!(error.kind(), io::ErrorKind::PermissionDenied);
}
