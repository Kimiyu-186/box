//! Direct filesystem modes checked through real agent, tool, and local-server execution.

#![cfg(feature = "test-support")]

use std::path::PathBuf;
use std::process::{Command, Output};

#[path = "support/bounded_output.rs"]
mod bounded_output;
#[path = "support/fixture.rs"]
mod fixture;

struct Fixture {
    home: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = fixture::short_temporary_home();
        let root = home.path().canonicalize().unwrap();
        let data = root.join("data");
        std::fs::create_dir_all(data.join("nested")).unwrap();
        std::fs::create_dir(root.join("bin")).unwrap();
        for name in ["mode-agent", "mode-tool", "mode-server"] {
            std::fs::copy(
                env!("CARGO_BIN_EXE_box-filesystem-probe"),
                root.join("bin").join(name),
            )
            .unwrap();
        }
        std::fs::write(data.join("nested/value"), "seed").unwrap();
        std::fs::write(
            root.join("policy.dw"),
            "permit(principal, action, resource);",
        )
        .unwrap();
        Self { home, root, data }
    }

    fn grants(&self, shape: &str, read: bool, write: bool) -> String {
        let file = self.data.join("nested/value");
        let (read_key, read_path, write_key, write_path) = match shape {
            "file" => ("read_file", file.clone(), "write_file", file),
            "file-in-read-tree" => ("read", self.data.clone(), "write_file", file),
            "nested-directory" => ("read", self.data.clone(), "write", self.data.join("nested")),
            "partial-read" => ("read_file", file, "write", self.data.clone()),
            "directory" => ("read", self.data.clone(), "write", self.data.clone()),
            _ => unreachable!(),
        };
        let mut grants = String::new();
        if read {
            grants += &format!("{read_key} = [{:?}]\n", read_path.display().to_string());
        }
        if write {
            grants += &format!("{write_key} = [{:?}]\n", write_path.display().to_string());
        }
        grants
    }

    fn run(&self, role: &str, shape: &str, mode: &str) -> Output {
        let agent = self.root.join("bin/mode-agent").display().to_string();
        let target = self.data.join("nested/value").display().to_string();
        let command = match role {
            "agent" => vec![agent, mode.to_string(), target.clone()],
            "tool" => vec![
                agent,
                "tool-client".to_string(),
                mode.to_string(),
                target.clone(),
            ],
            "server" => vec![agent, "server-client".to_string()],
            _ => unreachable!(),
        };
        let grants = self.grants(shape, mode != "write-only", mode != "read-only");
        let process = match role {
            "agent" => format!("[agent.filesystem]\n{grants}"),
            "tool" => {
                format!("[tool.probe]\ncommand = ['mode-tool']\n[tool.probe.filesystem]\n{grants}")
            }
            "server" => format!(
                "[mcp.probe]\ntype = 'stdio'\ncommand = ['mode-server', 'server', {mode:?}, {target:?}]\n[mcp.probe.filesystem]\n{grants}"
            ),
            _ => unreachable!(),
        };
        let config = format!(
            "name = 'filesystem-modes'\nbox_dir = {:?}\npolicy = 'policy.dw'\n\
             [agent]\ncommand = {command:?}\nworkspace = {:?}\n{process}",
            self.root.join("state").display().to_string(),
            self.data.display().to_string(),
        );
        let path = self.root.join("box.toml");
        std::fs::write(&path, config).unwrap();
        let search = std::env::join_paths(
            [self.root.join("bin")]
                .into_iter()
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let mut command = Command::new(fixture::box_binary());
        command
            .args(["run", "--config"])
            .arg(path)
            .env("HOME", self.home.path())
            .env("PATH", search);
        bounded_output::bounded_output(command)
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_rejects_write_only_before_any_process_runs() {
    for role in ["agent", "tool", "server"] {
        for shape in [
            "file",
            "directory",
            "nested-directory",
            "file-in-read-tree",
            "partial-read",
        ] {
            let fixture = Fixture::new();
            let mode = if shape == "partial-read" {
                "read-write"
            } else {
                "write-only"
            };
            let output = fixture.run(role, shape, mode);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(!output.status.success(), "{role}/{shape}: {output:?}");
            assert!(
                stderr.contains("Linux cannot enforce write-only"),
                "{role}/{shape}: {output:?}"
            );
            let table = match role {
                "agent" => "[agent]",
                "tool" => "[tool.probe]",
                _ => "[mcp.probe]",
            };
            assert!(stderr.contains(table), "{role}/{shape}: {output:?}");
            assert!(!stderr.contains("FILESYSTEM_PROBE_STARTED"), "{output:?}");
            assert!(!String::from_utf8_lossy(&output.stdout).contains("FILESYSTEM_PROBE_OK"));
            assert_eq!(
                std::fs::read_to_string(fixture.data.join("nested/value")).unwrap(),
                "seed"
            );
        }
    }
}

#[test]
fn explicit_read_write_succeeds_for_every_process_and_scope() {
    for role in ["agent", "tool", "server"] {
        for shape in ["file", "directory", "nested-directory", "file-in-read-tree"] {
            let fixture = Fixture::new();
            let output = fixture.run(role, shape, "read-write");
            assert!(output.status.success(), "{role}/{shape}: {output:?}");
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("FILESYSTEM_PROBE_OK"),
                "{role}/{shape}: {output:?}"
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("(read-write)"),
                "{role}/{shape}: {output:?}"
            );
            assert_eq!(
                std::fs::read_to_string(fixture.data.join("nested/value")).unwrap(),
                "changed"
            );
        }
    }
}

#[test]
fn read_only_preserves_contents_for_every_process() {
    for role in ["agent", "tool", "server"] {
        let fixture = Fixture::new();
        let output = fixture.run(role, "file", "read-only");
        assert!(output.status.success(), "{role}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("FILESYSTEM_PROBE_OK"),
            "{role}: {output:?}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("(read-only)"),
            "{role}: {output:?}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.data.join("nested/value")).unwrap(),
            "seed"
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn macos_write_only_preserves_content_read_refusal() {
    for role in ["agent", "tool", "server"] {
        let fixture = Fixture::new();
        let output = fixture.run(role, "file", "write-only");
        assert!(output.status.success(), "{role}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("FILESYSTEM_PROBE_OK"),
            "{role}: {output:?}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("(write-only)"),
            "{role}: {output:?}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.data.join("nested/value")).unwrap(),
            "changed"
        );
    }
}
