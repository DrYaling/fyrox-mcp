//! MCP tool discovery and lifecycle owned by the EditorPlugin.
//!
//! The plugin starts the stdio MCP tool after the Editor transport is ready,
//! so individual Editor binaries only need to register `McpEditorPlugin`.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

const MCP_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6501);
const MCP_NAMES: &[&str] = &["mcp-tool.exe", "mcp-bridge.exe", "mcp-tool", "mcp-bridge"];

pub fn start() -> Option<Child> {
    if std::env::var_os("MCP_TOOL_AUTOSTART")
        .is_some_and(|value| value == "0" || value.eq_ignore_ascii_case("false"))
    {
        eprintln!("MCP tool auto-start disabled by MCP_TOOL_AUTOSTART");
        return None;
    }
    if TcpStream::connect_timeout(&MCP_ADDR, Duration::from_millis(100)).is_ok() {
        eprintln!("MCP bridge already reachable on {MCP_ADDR}");
        return None;
    }
    let roots = search_roots();
    let candidates = candidate_paths(&roots);
    let project_root = project_root(&roots);
    for path in &candidates {
        if !path.is_file() {
            continue;
        }
        match Command::new(path)
            .current_dir(&project_root)
            .stdin(Stdio::piped())
            .spawn()
        {
            Ok(child) => {
                eprintln!("MCP tool started: {}", path.display());
                return Some(child);
            }
            Err(error) => eprintln!("MCP tool start failed ({}): {error}", path.display()),
        }
    }
    eprintln!(
        "MCP tool not found; searched {} locations. Set MCP_TOOL_BIN or MCP_BRIDGE_BIN to override.",
        candidates.len()
    );
    None
}

fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        push_root_chain(&mut roots, cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            push_root_chain(&mut roots, parent.to_owned());
        }
    }
    let existing = roots.clone();
    for root in existing {
        if let Some(parent) = root.parent() {
            for sibling in [parent.join("Fyrox/fwok"), parent.join("fyrox/fwok")] {
                if sibling.is_dir() && !roots.iter().any(|item| item == &sibling) {
                    roots.push(sibling);
                }
            }
        }
    }
    roots
}

fn push_root_chain(roots: &mut Vec<PathBuf>, start: PathBuf) {
    let mut current = Some(start);
    for _ in 0..6 {
        let Some(path) = current else { break };
        if path.is_dir() && !roots.iter().any(|item| item == &path) {
            roots.push(path.clone());
        }
        current = path.parent().map(Path::to_path_buf);
    }
}

fn candidate_paths(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Some(path) = std::env::var_os("MCP_TOOL_BIN")
        .or_else(|| std::env::var_os("MCP_BRIDGE_BIN"))
        .map(PathBuf::from)
    {
        result.push(path);
    }
    for root in roots {
        for relative in [
            "mcp/target/debug",
            "mcp/target/release",
            "mcp/mcp-bridge/target/debug",
            "mcp/mcp-bridge/target/release",
            "target/debug",
            "target/release",
            "data/mcp",
            "data/editor",
            "data",
        ] {
            for name in MCP_NAMES {
                let path = root.join(relative).join(name);
                if !result.iter().any(|item| item == &path) {
                    result.push(path);
                }
            }
        }
    }
    result
}

fn project_root(roots: &[PathBuf]) -> PathBuf {
    roots
        .iter()
        .find(|root| root.join("Cargo.toml").is_file() && root.join("data").is_dir())
        .cloned()
        .or_else(|| roots.first().cloned())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_include_project_and_mcp_build_locations() {
        let roots = vec![PathBuf::from("C:/project")];
        let paths = candidate_paths(&roots);
        assert!(paths.iter().any(|p| p.ends_with("data/mcp/mcp-tool.exe")));
        assert!(paths
            .iter()
            .any(|p| p.ends_with("data/editor/mcp-bridge.exe")));
        assert!(paths
            .iter()
            .any(|p| p.ends_with("mcp/target/debug/mcp-bridge.exe")));
    }

    #[test]
    fn compiled_tool_is_preferred_over_deployed_copy() {
        let paths = candidate_paths(&[PathBuf::from("C:/project")]);
        let compiled = paths
            .iter()
            .position(|path| path.ends_with("target/debug/mcp-bridge.exe"))
            .expect("compiled candidate");
        let deployed = paths
            .iter()
            .position(|path| path.ends_with("data/editor/mcp-bridge.exe"))
            .expect("deployed candidate");
        assert!(compiled < deployed);
    }
}
