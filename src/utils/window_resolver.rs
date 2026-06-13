use crate::error::{Result, WlsnipError};
use crate::utils::geometry::Region;
use serde::Deserialize;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub pid: Option<u32>,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub region: Region,
}

pub fn resolve_window(query: Option<&str>, ignore_apps: Option<&[String]>) -> Result<Region> {
    let windows = get_all_windows()?;

    if windows.is_empty() {
        return Err(WlsnipError::Capture("No windows found via IPC".to_string()));
    }

    if let Some(q) = query {
        let q_lower = q.to_lowercase();
        
        // Match by PID first if query is a number
        if let Ok(target_pid) = q.parse::<u32>() {
            if let Some(win) = windows.iter().find(|w| w.pid == Some(target_pid)) {
                return Ok(win.region.clone());
            }
        }

        // Match by app_id or title
        let matched = windows.iter().find(|w| {
            w.app_id.as_deref().map(|id| id.to_lowercase().contains(&q_lower)).unwrap_or(false)
                || w.title.as_deref().map(|t| t.to_lowercase().contains(&q_lower)).unwrap_or(false)
        });

        if let Some(win) = matched {
            Ok(win.region.clone())
        } else {
            Err(WlsnipError::Capture(format!("No window matching '{}' found", q)))
        }
    } else {
        // No query: try to get active window directly (or first valid window that is not ignored)
        if let Ok(active) = get_active_window(&windows, ignore_apps) {
            return Ok(active.region);
        }
        Err(WlsnipError::Capture("Could not determine active window".to_string()))
    }
}

// ── IPC Parsers ────────────────────────────────────────────────────────────

fn get_all_windows() -> Result<Vec<WindowInfo>> {
    // Try mmsg (Mangowc)
    if let Ok(output) = Command::new("mmsg").args(["get", "all-clients"]).output() {
        if output.status.success() {
            if let Ok(wins) = parse_mmsg_clients(&output.stdout) {
                return Ok(wins);
            }
        }
    }

    // Try hyprctl (Hyprland)
    if let Ok(output) = Command::new("hyprctl").args(["clients", "-j"]).output() {
        if output.status.success() {
            if let Ok(wins) = parse_hyprctl_clients(&output.stdout) {
                return Ok(wins);
            }
        }
    }

    // Try swaymsg (Sway)
    if let Ok(output) = Command::new("swaymsg").args(["-t", "get_tree"]).output() {
        if output.status.success() {
            if let Ok(wins) = parse_swaymsg_tree(&output.stdout) {
                return Ok(wins);
            }
        }
    }

    Ok(Vec::new()) // No supported IPC found
}

fn get_active_window(windows: &[WindowInfo], ignore_apps: Option<&[String]>) -> Result<WindowInfo> {
    let mut candidate = None;

    // Try mmsg
    if let Ok(output) = Command::new("mmsg").args(["get", "focusing-client"]).output() {
        if output.status.success() {
            if let Ok(win) = parse_mmsg_client(&output.stdout) {
                candidate = Some(win);
            }
        }
    }

    // Try hyprctl
    if candidate.is_none() {
        if let Ok(output) = Command::new("hyprctl").args(["activewindow", "-j"]).output() {
            if output.status.success() {
                if let Ok(win) = parse_hyprctl_activewindow(&output.stdout) {
                    candidate = Some(win);
                }
            }
        }
    }

    // If candidate is ignored, we must fallback to the top window in `windows` list that is not ignored.
    // Sway active window is tricky, so we just use the windows list.
    
    if let Some(win) = candidate {
        if !is_ignored(&win, ignore_apps) {
            return Ok(win);
        }
    }

    // Fallback: pick the first window in the tree that is not ignored
    for win in windows {
        if !is_ignored(win, ignore_apps) {
            return Ok(win.clone());
        }
    }

    Err(WlsnipError::Capture("Failed to get a valid active window".to_string()))
}

fn is_ignored(win: &WindowInfo, ignore_apps: Option<&[String]>) -> bool {
    let Some(ignores) = ignore_apps else { return false; };
    if ignores.is_empty() { return false; }
    
    if let Some(app_id) = &win.app_id {
        let app_id_lower = app_id.to_lowercase();
        return ignores.iter().any(|ig| app_id_lower.contains(&ig.to_lowercase()));
    }
    false
}

// ── Mangowc (mmsg) ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct MmsgResponse {
    clients: Vec<MmsgClient>,
}

#[derive(Deserialize)]
struct MmsgClient {
    id: Option<u32>, // Used as PID fallback or similar if pid is not exposed
    pid: Option<u32>,
    #[serde(rename = "appid")]
    app_id: Option<String>,
    title: Option<String>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn parse_mmsg_client(data: &[u8]) -> Result<WindowInfo> {
    let client: MmsgClient = serde_json::from_slice(data)
        .map_err(|e| WlsnipError::Capture(format!("Failed to parse mmsg: {}", e)))?;
    
    Ok(WindowInfo {
        pid: client.pid.or(client.id), // Fallback if mmsg provides id instead of pid
        app_id: client.app_id,
        title: client.title,
        region: Region {
            x: client.x,
            y: client.y,
            width: client.width,
            height: client.height,
        },
    })
}

fn parse_mmsg_clients(data: &[u8]) -> Result<Vec<WindowInfo>> {
    let res: MmsgResponse = serde_json::from_slice(data)
        .map_err(|e| WlsnipError::Capture(format!("Failed to parse mmsg: {}", e)))?;
    
    Ok(res.clients.into_iter().map(|c| WindowInfo {
        pid: c.pid.or(c.id),
        app_id: c.app_id,
        title: c.title,
        region: Region {
            x: c.x,
            y: c.y,
            width: c.width,
            height: c.height,
        },
    }).collect())
}

// ── Hyprland (hyprctl) ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct HyprctlClient {
    pid: Option<u32>,
    class: Option<String>,
    title: Option<String>,
    at: [i32; 2],
    size: [u32; 2],
}

fn parse_hyprctl_activewindow(data: &[u8]) -> Result<WindowInfo> {
    let client: HyprctlClient = serde_json::from_slice(data)
        .map_err(|e| WlsnipError::Capture(format!("Failed to parse hyprctl: {}", e)))?;
    
    Ok(WindowInfo {
        pid: client.pid,
        app_id: client.class,
        title: client.title,
        region: Region {
            x: client.at[0],
            y: client.at[1],
            width: client.size[0],
            height: client.size[1],
        },
    })
}

fn parse_hyprctl_clients(data: &[u8]) -> Result<Vec<WindowInfo>> {
    let clients: Vec<HyprctlClient> = serde_json::from_slice(data)
        .map_err(|e| WlsnipError::Capture(format!("Failed to parse hyprctl: {}", e)))?;
    
    Ok(clients.into_iter().map(|c| WindowInfo {
        pid: c.pid,
        app_id: c.class,
        title: c.title,
        region: Region {
            x: c.at[0],
            y: c.at[1],
            width: c.size[0],
            height: c.size[1],
        },
    }).collect())
}

// ── Sway (swaymsg) ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SwayNode {
    pid: Option<u32>,
    app_id: Option<String>,
    name: Option<String>,
    rect: SwayRect,
    nodes: Vec<SwayNode>,
    floating_nodes: Vec<SwayNode>,
}

#[derive(Deserialize)]
struct SwayRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn parse_swaymsg_tree(data: &[u8]) -> Result<Vec<WindowInfo>> {
    let root: SwayNode = serde_json::from_slice(data)
        .map_err(|e| WlsnipError::Capture(format!("Failed to parse swaymsg: {}", e)))?;
    
    let mut windows = Vec::new();
    fn extract_windows(node: &SwayNode, out: &mut Vec<WindowInfo>) {
        if node.app_id.is_some() || node.pid.is_some() {
            out.push(WindowInfo {
                pid: node.pid,
                app_id: node.app_id.clone(),
                title: node.name.clone(),
                region: Region {
                    x: node.rect.x,
                    y: node.rect.y,
                    width: node.rect.width,
                    height: node.rect.height,
                },
            });
        }
        for child in &node.nodes {
            extract_windows(child, out);
        }
        for child in &node.floating_nodes {
            extract_windows(child, out);
        }
    }
    
    extract_windows(&root, &mut windows);
    Ok(windows)
}
