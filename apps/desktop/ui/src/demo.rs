//! Browser demo backend: lets `trunk serve` show the real UI in any browser (design reviews,
//! screenshots) with a few bundled examples and no renderer.

use dioramic_desktop_ipc::{FileKind, FileNode, PreviewRequest, PreviewResponse, PreviewStatus, Workspace};

/// Fake root of the demo workspace.
pub const ROOT: &str = "/demo/examples";

const FILES: [(&str, &str); 4] = [
    ("checkout.diorama.yaml", include_str!("../../../../examples/checkout.diorama.yaml")),
    ("event-driven.diorama.yaml", include_str!("../../../../examples/event-driven.diorama.yaml")),
    ("kubernetes-platform.diorama.yaml", include_str!("../../../../examples/kubernetes-platform.diorama.yaml")),
    ("minimal.diorama.yaml", include_str!("../../../../examples/minimal.diorama.yaml")),
];

fn path_of(name: &str) -> String {
    format!("{ROOT}/{name}")
}

/// The demo folder.
pub fn workspace() -> Workspace {
    let tree = FILES
        .iter()
        .map(|(name, _)| FileNode {
            name: (*name).to_owned(),
            path: path_of(name),
            kind: FileKind::Spec,
            children: Vec::new(),
        })
        .collect();
    Workspace { root: ROOT.to_owned(), name: "examples".to_owned(), tree }
}

/// Text of a demo file.
pub fn read(path: &str) -> Result<String, String> {
    FILES
        .iter()
        .find(|(name, _)| path_of(name) == path)
        .map(|(_, text)| (*text).to_owned())
        .ok_or_else(|| format!("no such demo file: {path}"))
}

/// The browser has no engine.
pub fn preview(_request: &PreviewRequest) -> PreviewResponse {
    PreviewResponse {
        status: PreviewStatus::EngineUnavailable { reason: "Renderer not available in the browser preview".to_owned() },
        svg: None,
        diagnostics: Vec::new(),
        view: None,
        notes: Vec::new(),
        elapsed_ms: 0,
    }
}
