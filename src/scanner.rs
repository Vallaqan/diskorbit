use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct FolderNode {
    pub name:       String,
    pub full_path:  String,
    pub is_file:    bool,
    pub size_bytes: u64,
    pub percentage: f32,
    pub children:   Vec<FolderNode>,
}

impl FolderNode {
    pub fn size_display(&self) -> String { fmt_bytes(self.size_bytes) }
}

pub enum ScanMsg {
    Progress(String),
    Done(FolderNode),
    Error(String),
}

pub fn start_scan(root: String, tx: Sender<ScanMsg>, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let _ = tx.send(ScanMsg::Progress(format!("Scanning {}…", root)));
        let counter = Arc::new(AtomicU64::new(0));

        // Use rayon for parallel scanning of subdirectories
        match scan_parallel(Path::new(&root), &tx, &cancel, &counter) {
            Some(node) => { let _ = tx.send(ScanMsg::Done(node)); }
            None       => { let _ = tx.send(ScanMsg::Error("Scan cancelled.".into())); }
        }
    });
}

fn scan_parallel(
    path:    &Path,
    tx:      &Sender<ScanMsg>,
    cancel:  &Arc<AtomicBool>,
    counter: &Arc<AtomicU64>,
) -> Option<FolderNode> {
    if cancel.load(Ordering::Relaxed) { return None; }

    let name = path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let full_path = path.to_string_lossy().into_owned();

    let count = counter.fetch_add(1, Ordering::Relaxed);
    if count % 200 == 0 {
        let _ = tx.send(ScanMsg::Progress(format!("Scanning  {}", full_path)));
    }

    let entries = match std::fs::read_dir(path) {
        Ok(e)  => e,
        Err(_) => return Some(FolderNode {
            name, full_path, is_file: false, size_bytes: 0, percentage: 0.0, children: vec![],
        }),
    };

    // Collect all entries first so we can parallelise directories
    let mut file_nodes:  Vec<FolderNode> = Vec::new();
    let mut dir_paths:   Vec<std::path::PathBuf> = Vec::new();

    for entry in entries.flatten() {
        if cancel.load(Ordering::Relaxed) { return None; }
        let child_path = entry.path();
        // Use symlink_metadata so we never follow symlinks —
        // following them can cause infinite loops on Windows junctions.
        let Ok(meta) = child_path.symlink_metadata() else { continue };
        if meta.file_type().is_symlink() { continue; }

        if meta.is_file() {
            let size = meta.len();
            file_nodes.push(FolderNode {
                name: child_path.file_name().unwrap_or_default()
                    .to_string_lossy().into_owned(),
                full_path:  child_path.to_string_lossy().into_owned(),
                is_file:    true,
                size_bytes: size,
                percentage: 0.0,
                children:   vec![],
            });
        } else if meta.is_dir() {
            dir_paths.push(child_path);
        }
    }

    // Scan subdirectories in parallel using rayon
    use rayon::prelude::*;

    let tx_ref      = tx;
    let cancel_ref  = cancel;
    let counter_ref = counter;

    // Each rayon thread gets clones of the shared state
    let dir_results: Vec<Option<FolderNode>> = dir_paths
        .into_par_iter()
        .map(|dir_path| {
            scan_parallel(&dir_path, tx_ref, cancel_ref, counter_ref)
        })
        .collect();

    // Bail if any returned None (cancellation)
    let mut dir_nodes: Vec<FolderNode> = Vec::new();
    for result in dir_results {
        match result {
            Some(node) => dir_nodes.push(node),
            None       => return None,
        }
    }

    let mut children: Vec<FolderNode> = file_nodes;
    children.extend(dir_nodes);

    let total_size: u64 = children.iter().map(|c| c.size_bytes).sum();

    // Sort largest first
    children.sort_unstable_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

    // Calculate percentages relative to this directory
    if total_size > 0 {
        for child in &mut children {
            child.percentage = child.size_bytes as f32 / total_size as f32 * 100.0;
        }
    }

    Some(FolderNode {
        name,
        full_path,
        is_file:    false,
        size_bytes: total_size,
        percentage: 0.0,
        children,
    })
}

pub fn fmt_bytes(bytes: u64) -> String {
    match bytes {
        b if b >= 1_073_741_824 => format!("{:.1} GB", b as f64 / 1_073_741_824.0),
        b if b >= 1_048_576     => format!("{:.0} MB", b as f64 / 1_048_576.0),
        b if b >= 1_024         => format!("{:.0} KB", b as f64 / 1_024.0),
        b                       => format!("{} B",     b),
    }
}
