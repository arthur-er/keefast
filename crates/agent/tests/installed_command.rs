//! Where Agents find `keefast-agent`: next to keefast, or for an AppImage, mounted somewhere new
//! each run, a copy the desktop app keeps in its data directory (spec story 83).

use std::fs;

use keefast_agent::installed::AgentCommand;

#[test]
fn a_command_shipped_somewhere_transient_is_copied_where_agents_find_it_and_kept_current() {
    let mount = tempfile::tempdir().unwrap();
    let shipped = mount.path().join("keefast-agent");
    fs::write(&shipped, "keefast-agent 1").unwrap();
    let data = tempfile::tempdir().unwrap();
    let copy = data.path().join("keefast").join("keefast-agent");
    let command = AgentCommand::copied_from(shipped.clone(), copy.clone());

    command.install().unwrap();
    assert_eq!(command.path(), copy);
    assert_eq!(fs::read_to_string(&copy).unwrap(), "keefast-agent 1");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&copy).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "the copy is executable");
    }

    // A newer AppImage replaces the copy when keefast starts again.
    fs::write(&shipped, "keefast-agent 2").unwrap();
    command.install().unwrap();
    assert_eq!(fs::read_to_string(&copy).unwrap(), "keefast-agent 2");
}

#[test]
fn a_command_installed_by_a_package_is_left_where_it_is() {
    let bin = tempfile::tempdir().unwrap();
    let installed = bin.path().join("keefast-agent");
    fs::write(&installed, "keefast-agent").unwrap();
    let command = AgentCommand::new(installed.clone());

    command.install().unwrap();

    assert_eq!(command.path(), installed);
    assert_eq!(fs::read_dir(bin.path()).unwrap().count(), 1);
}
