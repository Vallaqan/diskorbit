use eframe::egui;
use egui::{Color32, FontId, RichText, Stroke, Vec2};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use crate::scanner::{fmt_bytes, start_scan, FolderNode, ScanMsg};

// ── Palette ───────────────────────────────────────────────────────────────────
const BG_DEEP:      Color32 = Color32::from_rgb(0x08, 0x0C, 0x12);
const BG_PANEL:     Color32 = Color32::from_rgb(0x0F, 0x17, 0x2A);
const BG_HEADER:    Color32 = Color32::from_rgb(0x0A, 0x0F, 0x1A);
const BG_HOVER:     Color32 = Color32::from_rgb(0x1A, 0x25, 0x40);
const BG_SELECTED:  Color32 = Color32::from_rgb(0x1E, 0x2D, 0x45);
const BORDER:       Color32 = Color32::from_rgb(0x1E, 0x29, 0x3B);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xCB, 0xD5, 0xE1);
const TEXT_MUTED:   Color32 = Color32::from_rgb(0x94, 0xA3, 0xB8);
const TEXT_DIM:     Color32 = Color32::from_rgb(0x47, 0x55, 0x69);
const ACCENT:       Color32 = Color32::from_rgb(0xF9, 0x73, 0x16);
const GREEN:        Color32 = Color32::from_rgb(0x34, 0xD3, 0x99);
const BAR_BG:       Color32 = Color32::from_rgb(0x1E, 0x29, 0x3B);

// Right-column layout — shared between draw_node and show_column_headers
const PCT_W:  f32 = 45.0;
const SIZE_W: f32 = 85.0;
const BAR_W:  f32 = 90.0;
const COL_GAP: f32 = 12.0;
const RIGHT_MARGIN: f32 = 16.0;

// ── App state ─────────────────────────────────────────────────────────────────
pub struct DiskOrbitApp {
    drives:         Vec<String>,
    selected_drive: usize,
    scan_result:    Option<FolderNode>,
    scan_rx:        Option<mpsc::Receiver<ScanMsg>>,
    cancel_flag:    Arc<AtomicBool>,
    is_scanning:    bool,
    status:         String,
    footer:         String,
    used_bytes:     u64,
    free_bytes:     u64,
    total_bytes:    u64,
    expanded:       HashSet<String>,
    is_admin:       bool,
    custom_path:    Option<String>,
    browse_rx:      Option<mpsc::Receiver<Option<std::path::PathBuf>>>,
    scan_root:      String,
}

impl DiskOrbitApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        apply_theme(&cc.egui_ctx);
        Self {
            drives:         available_drives(),
            selected_drive: 0,
            scan_result:    None,
            scan_rx:        None,
            cancel_flag:    Arc::new(AtomicBool::new(false)),
            is_scanning:    false,
            status:         "Select a drive or browse to a folder, then click Scan.".into(),
            footer:         String::new(),
            used_bytes:     0,
            free_bytes:     0,
            total_bytes:    0,
            expanded:       HashSet::new(),
            is_admin:       is_admin(),
            custom_path:    None,
            browse_rx:      None,
            scan_root:      String::new(),
        }
    }

    fn do_scan(&mut self) {
        let root = if let Some(ref p) = self.custom_path {
            p.clone()
        } else {
            if self.drives.is_empty() { return; }
            self.drives[self.selected_drive].clone()
        };

        self.cancel_flag.store(true, Ordering::Relaxed);
        self.scan_result = None;
        self.expanded.clear();
        self.is_scanning = true;
        self.status      = format!("Scanning {}…", root);
        self.footer      = String::new();
        self.scan_root   = root.clone();

        if self.custom_path.is_none() {
            if let Some((used, free, total)) = drive_usage(&root) {
                self.used_bytes  = used;
                self.free_bytes  = free;
                self.total_bytes = total;
            }
        } else {
            self.used_bytes  = 0;
            self.free_bytes  = 0;
            self.total_bytes = 0;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel_flag = Arc::clone(&cancel);

        let (tx, rx) = mpsc::channel();
        self.scan_rx  = Some(rx);
        start_scan(root, tx, cancel);
    }

    fn do_browse(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.browse_rx = Some(rx);
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new().pick_folder();
            let _ = tx.send(picked);
        });
    }

    fn poll_browse(&mut self) {
        let Some(rx) = &self.browse_rx else { return };
        if let Ok(result) = rx.try_recv() {
            if let Some(path) = result {
                let path_str = path.to_string_lossy().into_owned();
                // Normalise to trailing backslash so we can compare against the drives list
                let normalised = if path_str.ends_with('\\') || path_str.ends_with('/') {
                    path_str.clone()
                } else {
                    format!("{}\\", path_str)
                };
                if let Some(idx) = self.drives.iter().position(|d| *d == normalised) {
                    // User picked a drive root — just select it in the combo
                    self.selected_drive = idx;
                    self.custom_path = None;
                } else {
                    self.custom_path = Some(path_str);
                }
            }
            self.browse_rx = None;
        }
    }

    fn do_cancel(&mut self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
        self.is_scanning = false;
        self.status      = "Cancelled.".into();
    }

    fn poll_scan(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.scan_rx else { return };

        loop {
            match rx.try_recv() {
                Ok(ScanMsg::Progress(s)) => {
                    self.status = s;
                    ctx.request_repaint();
                }
                Ok(ScanMsg::Done(mut node)) => {
                    if self.total_bytes > 0 {
                        node.percentage =
                            self.used_bytes as f32 / self.total_bytes as f32 * 100.0;
                    }
                    self.footer      = format!(
                        "{}  ·  {} in {} items",
                        self.scan_root,
                        fmt_bytes(node.size_bytes),
                        count_items(&node),
                    );
                    self.status      = "Done.".into();
                    self.is_scanning = false;
                    self.scan_result = Some(node);
                    self.scan_rx     = None;
                    ctx.request_repaint();
                    break;
                }
                Ok(ScanMsg::Error(e)) => {
                    self.status      = e;
                    self.is_scanning = false;
                    self.scan_rx     = None;
                    ctx.request_repaint();
                    break;
                }
                Err(mpsc::TryRecvError::Empty)        => break,
                Err(mpsc::TryRecvError::Disconnected) => { self.scan_rx = None; break; }
            }
        }

        if self.is_scanning {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }
    }
}

// ── eframe App ────────────────────────────────────────────────────────────────
impl eframe::App for DiskOrbitApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_scan(ctx);
        self.poll_browse();

        // Status bar
        egui::TopBottomPanel::bottom("statusbar")
            .frame(panel_frame(BG_DEEP))
            .show(ctx, |ui: &mut egui::Ui| {
                ui.horizontal(|ui: &mut egui::Ui| {
                    if self.is_scanning { ui.spinner(); }
                    ui.label(RichText::new(&self.status).color(TEXT_MUTED).font(mono(11.0)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                        ui.label(RichText::new(&self.footer).color(TEXT_MUTED).font(mono(11.0)));
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_DEEP))
            .show(ctx, |ui: &mut egui::Ui| {
                self.show_toolbar(ui);
                self.show_admin_banner(ui);
                self.show_column_headers(ui);
                self.show_tree(ui);
            });
    }
}

// ── Panel renderers ───────────────────────────────────────────────────────────
impl DiskOrbitApp {
    fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(BG_DEEP)
            .inner_margin(egui::Margin { left: 20.0, right: 20.0, top: 14.0, bottom: 14.0 })
            .stroke(Stroke::new(1.0, BORDER))
            .show(ui, |ui: &mut egui::Ui| {
                ui.horizontal(|ui: &mut egui::Ui| {

                    // Logo
                    ui.vertical(|ui: &mut egui::Ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.horizontal(|ui: &mut egui::Ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(RichText::new("DISK").color(ACCENT).font(bold(20.0)));
                            ui.label(RichText::new("ORBIT").color(Color32::from_rgb(0xFB, 0x92, 0x3C)).font(bold(20.0)));
                        });
                        ui.label(RichText::new("STORAGE ANALYSIS SYSTEM").color(TEXT_DIM).font(mono(8.0)));
                    });

                    // Divider
                    ui.add_space(16.0);
                    let divider_rect = egui::Rect::from_min_size(
                        egui::pos2(ui.cursor().min.x, ui.cursor().min.y),
                        Vec2::new(1.0, 36.0),
                    );
                    ui.painter().rect_filled(divider_rect, 0.0, BORDER);
                    ui.add_space(17.0);

                    // Drive selector + scan controls, vertically centred
                    ui.vertical_centered_justified(|ui: &mut egui::Ui| {
                        ui.horizontal(|ui: &mut egui::Ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;

                            // Drive combo — greyed out when a custom browse path overrides it
                            ui.add_enabled_ui(self.custom_path.is_none(), |ui: &mut egui::Ui| {
                                egui::ComboBox::from_id_source("drives")
                                    .width(100.0)
                                    .selected_text(
                                        self.drives.get(self.selected_drive)
                                            .map(|s| s.as_str()).unwrap_or("—"),
                                    )
                                    .show_ui(ui, |ui: &mut egui::Ui| {
                                        for (i, drive) in self.drives.iter().enumerate() {
                                            if ui.selectable_value(&mut self.selected_drive, i, drive.clone()).clicked() {
                                                self.custom_path = None;
                                            }
                                        }
                                    });
                            });

                            // BROWSE
                            let is_browsing = self.browse_rx.is_some();
                            if ui.add_enabled(
                                !is_browsing && !self.is_scanning,
                                egui::Button::new(
                                    RichText::new("BROWSE").color(TEXT_MUTED).font(mono(12.0))
                                )
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::new(1.0, BORDER))
                                .rounding(4.0),
                            ).clicked() {
                                self.do_browse();
                            }

                            // Custom path indicator — shown when a folder was picked via browse
                            let custom = self.custom_path.clone();
                            if let Some(ref path) = custom {
                                let display = if path.len() > 40 {
                                    format!("…{}", &path[path.len()-38..])
                                } else {
                                    path.clone()
                                };
                                ui.label(RichText::new(format!("📁 {}", display)).color(ACCENT).font(mono(11.0)));
                                if ui.add(
                                    egui::Button::new(RichText::new("×").color(TEXT_MUTED).font(mono(11.0)))
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::new(1.0, BORDER))
                                        .rounding(4.0)
                                        .min_size(Vec2::new(22.0, 22.0)),
                                ).clicked() {
                                    self.custom_path = None;
                                }
                            }

                            // SCAN
                            let scan_enabled = !self.is_scanning;
                            if ui.add_enabled(
                                scan_enabled,
                                egui::Button::new(
                                    RichText::new("SCAN").color(Color32::BLACK).font(mono(12.0)).strong()
                                )
                                .fill(ACCENT)
                                .stroke(Stroke::NONE)
                                .rounding(4.0)
                                .min_size(Vec2::new(70.0, 0.0)),
                            ).clicked() {
                                self.do_scan();
                            }

                            // CANCEL — only while scanning
                            if self.is_scanning {
                                if ui.add(
                                    egui::Button::new(
                                        RichText::new("CANCEL").color(TEXT_MUTED).font(mono(11.0))
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::new(1.0, BORDER))
                                    .rounding(4.0),
                                ).clicked() {
                                    self.do_cancel();
                                }
                            }

                            // Drive usage — right-aligned, only for full-drive scans
                            if self.total_bytes > 0 && self.custom_path.is_none() {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                                    ui.label(RichText::new(fmt_bytes(self.total_bytes)).color(TEXT_PRIMARY).font(mono(11.0)));
                                    ui.label(RichText::new("TOTAL ").color(TEXT_DIM).font(mono(11.0)));
                                    ui.add_space(8.0);
                                    ui.label(RichText::new(fmt_bytes(self.free_bytes)).color(GREEN).font(mono(11.0)));
                                    ui.label(RichText::new("FREE ").color(TEXT_DIM).font(mono(11.0)));
                                    ui.add_space(8.0);
                                    ui.label(RichText::new(fmt_bytes(self.used_bytes)).color(ACCENT).font(mono(11.0)));
                                    ui.label(RichText::new("USED ").color(TEXT_DIM).font(mono(11.0)));
                                });
                            }
                        });
                    });
                });

            });
    }

    fn show_admin_banner(&self, ui: &mut egui::Ui) {
        if self.is_admin { return; }

        egui::Frame::none()
            .fill(Color32::from_rgb(0x3D, 0x22, 0x08))
            .inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 7.0, bottom: 7.0 })
            .show(ui, |ui: &mut egui::Ui| {
                ui.horizontal(|ui: &mut egui::Ui| {
                    ui.label(RichText::new("⚠").color(ACCENT).font(mono(12.0)));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Not running as administrator — some system folders will be inaccessible.")
                            .color(Color32::from_rgb(0xFB, 0xD3, 0xA0))
                            .font(mono(11.0)),
                    );
                });
            });
    }

    fn show_column_headers(&self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(BG_HEADER)
            .inner_margin(egui::Margin { left: 44.0, right: RIGHT_MARGIN, top: 6.0, bottom: 6.0 })
            .stroke(Stroke::new(1.0, BORDER))
            .show(ui, |ui: &mut egui::Ui| {
                // Paint headers using the same pixel math as draw_node
                let avail      = ui.available_width();
                let right      = avail;
                let pct_right  = right;
                let size_right = right - PCT_W - COL_GAP;
                let bar_right  = size_right - SIZE_W - COL_GAP;

                let (rect, _) = ui.allocate_exact_size(Vec2::new(avail, 18.0), egui::Sense::hover());
                let p = ui.painter();
                let cy = rect.center().y;

                p.text(egui::pos2(rect.min.x, cy), egui::Align2::LEFT_CENTER,
                    "NAME", mono(11.0), TEXT_MUTED);
                p.text(egui::pos2(rect.min.x + bar_right, cy), egui::Align2::RIGHT_CENTER,
                    "USAGE", mono(11.0), TEXT_MUTED);
                p.text(egui::pos2(rect.min.x + size_right, cy), egui::Align2::RIGHT_CENTER,
                    "SIZE", mono(11.0), TEXT_MUTED);
                p.text(egui::pos2(rect.min.x + pct_right, cy), egui::Align2::RIGHT_CENTER,
                    "%", mono(11.0), TEXT_MUTED);
            });
    }

    fn show_tree(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui: &mut egui::Ui| {
                ui.set_min_width(ui.available_width());

                if let Some(root) = self.scan_result.take() {
                    let mut expanded = std::mem::take(&mut self.expanded);
                    let root = draw_node(ui, root, 0, &mut expanded);
                    self.expanded    = expanded;
                    self.scan_result = Some(root);
                } else if self.is_scanning {
                    ui.add_space(60.0);
                    ui.vertical_centered(|ui: &mut egui::Ui| {
                        ui.spinner();
                        ui.add_space(12.0);
                        ui.label(RichText::new("Scanning, please wait…").color(TEXT_DIM).font(mono(13.0)));
                    });
                } else {
                    ui.add_space(60.0);
                    ui.vertical_centered(|ui: &mut egui::Ui| {
                        ui.label(RichText::new("Select a drive or browse to a folder, then click Scan").color(TEXT_DIM).font(mono(13.0)));
                    });
                }
            });
    }
}

// ── Tree node renderer ────────────────────────────────────────────────────────
fn draw_node(
    ui: &mut egui::Ui,
    mut node: FolderNode,
    depth: usize,
    expanded: &mut HashSet<String>,
) -> FolderNode {
    let is_expanded = expanded.contains(&node.full_path);
    let indent      = depth as f32 * 18.0;
    let avail_w     = ui.available_width();

    let (row_rect, resp) = ui.allocate_exact_size(Vec2::new(avail_w, 26.0), egui::Sense::click());

    if resp.hovered() {
        ui.painter().rect_filled(row_rect, 0.0, BG_HOVER);
    }

    let p     = ui.painter();
    let cy    = row_rect.center().y;
    let left  = row_rect.min.x;
    let right = row_rect.max.x - RIGHT_MARGIN;

    // Column anchor points (right edge of each column)
    let pct_right  = right;
    let size_right = pct_right  - PCT_W  - COL_GAP;
    let bar_right  = size_right - SIZE_W - COL_GAP;
    let bar_left   = bar_right  - BAR_W;

    // Expand arrow
    if !node.children.is_empty() {
        p.text(
            egui::pos2(left + indent + 4.0, cy),
            egui::Align2::LEFT_CENTER,
            if is_expanded { "▼" } else { "▶" },
            mono(9.0),
            if is_expanded { ACCENT } else { TEXT_DIM },
        );
    }

    // Icon + name — truncated to not overlap bar
    let name_x   = left + indent + 22.0;
    let max_name = (bar_left - COL_GAP - name_x).max(40.0);
    let icon     = if node.is_file { "📄 " } else { "📁 " };
    p.text(
        egui::pos2(name_x, cy),
        egui::Align2::LEFT_CENTER,
        clamp_text(&format!("{}{}", icon, node.name), max_name, ui, mono(13.0)),
        mono(13.0),
        TEXT_PRIMARY,
    );

    // Usage bar
    let bar_rect = egui::Rect::from_min_size(
        egui::pos2(bar_left, cy - 2.5),
        Vec2::new(BAR_W, 5.0),
    );
    p.rect_filled(bar_rect, 2.0, BAR_BG);
    if node.percentage > 0.0 {
        let mut fill = bar_rect;
        fill.max.x = bar_left + BAR_W * (node.percentage / 100.0).min(1.0);
        p.rect_filled(fill, 2.0, ACCENT);
    }

    // Size
    p.text(egui::pos2(size_right, cy), egui::Align2::RIGHT_CENTER,
        node.size_display(), mono(12.0), TEXT_MUTED);

    // Percentage
    p.text(egui::pos2(pct_right, cy), egui::Align2::RIGHT_CENTER,
        format!("{:.1}%", node.percentage), mono(11.0), TEXT_DIM);

    // Toggle expand on left-click
    if resp.clicked() && !node.children.is_empty() {
        if is_expanded { expanded.remove(&node.full_path); }
        else           { expanded.insert(node.full_path.clone()); }
    }

    // Right-click context menu
    resp.context_menu(|ui: &mut egui::Ui| {
        ui.set_min_width(180.0);
        let path = node.full_path.clone();
        let is_file = node.is_file;

        // Show the path truncated at the top as a label
        ui.label(
            RichText::new(if path.len() > 40 { format!("…{}", &path[path.len()-38..]) } else { path.clone() })
                .color(TEXT_DIM)
                .font(mono(10.0))
        );
        ui.separator();

        if ui.button("📂  Open in Explorer").clicked() {
            open_in_explorer(&path, is_file);
            ui.close_menu();
        }

        if ui.button("📋  Copy Path").clicked() {
            ui.output_mut(|o| o.copied_text = path.clone());
            ui.close_menu();
        }
    });

    // Recurse into children if expanded
    if is_expanded && !node.children.is_empty() {
        let children = std::mem::take(&mut node.children);
        node.children = children.into_iter()
            .map(|child| draw_node(ui, child, depth + 1, expanded))
            .collect();
    }

    node
}

// ── Helpers ───────────────────────────────────────────────────────────────────
fn mono(size: f32) -> FontId { FontId::monospace(size) }
fn bold(size: f32) -> FontId { FontId::new(size, egui::FontFamily::Proportional) }

fn clamp_text(s: &str, max_px: f32, ui: &egui::Ui, font: FontId) -> String {
    if max_px <= 0.0 { return String::new(); }
    let fonts = ui.fonts(|f| f.clone());
    let w = fonts.layout_no_wrap(s.to_string(), font.clone(), TEXT_PRIMARY).size().x;
    if w <= max_px { return s.to_string(); }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid  = (lo + hi + 1) / 2;
        let cand = chars[..mid].iter().collect::<String>() + "…";
        if fonts.layout_no_wrap(cand, font.clone(), TEXT_PRIMARY).size().x <= max_px {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

fn is_admin() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::mem;

        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn OpenProcessToken(
                process_handle: *mut std::ffi::c_void,
                desired_access: u32,
                token_handle: *mut *mut std::ffi::c_void,
            ) -> i32;
            fn GetTokenInformation(
                token_handle: *mut std::ffi::c_void,
                token_information_class: u32,
                token_information: *mut std::ffi::c_void,
                token_information_length: u32,
                return_length: *mut u32,
            ) -> i32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
        }

        unsafe {
            let mut token: *mut std::ffi::c_void = std::ptr::null_mut();
            // TOKEN_QUERY = 0x0008
            if OpenProcessToken(GetCurrentProcess(), 0x0008, &mut token) == 0 {
                return false;
            }
            // TokenElevation = 20
            let mut elevation: u32 = 0;
            let mut ret_len: u32   = 0;
            let ok = GetTokenInformation(
                token,
                20,
                &mut elevation as *mut u32 as *mut _,
                mem::size_of::<u32>() as u32,
                &mut ret_len,
            );
            CloseHandle(token);
            ok != 0 && elevation != 0
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

fn open_in_explorer(path: &str, is_file: bool) {
    #[cfg(target_os = "windows")]
    {
        let target = if is_file {
            // For files, open Explorer with the file selected
            std::process::Command::new("explorer.exe")
                .args(["/select,", path])
                .spawn()
        } else {
            // For directories, open Explorer at that directory
            std::process::Command::new("explorer.exe")
                .arg(path)
                .spawn()
        };
        let _ = target; // ignore spawn error
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (path, is_file);
    }
}

fn panel_frame(fill: Color32) -> egui::Frame {
    egui::Frame::none()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(16.0, 6.0))
        .stroke(Stroke::new(1.0, BORDER))
}

fn count_items(node: &FolderNode) -> usize {
    1 + node.children.iter().map(count_items).sum::<usize>()
}

fn available_drives() -> Vec<String> {
    #[cfg(target_os = "windows")]
    { ('A'..='Z').filter_map(|c| { let p = format!("{}:\\", c); std::path::Path::new(&p).exists().then_some(p) }).collect() }
    #[cfg(not(target_os = "windows"))]
    { vec!["/".to_string()] }
}

fn drive_usage(root: &str) -> Option<(u64, u64, u64)> {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = OsStr::new(root).encode_wide().chain(std::iter::once(0)).collect();
        let (mut fc, mut tot, mut ft) = (0u64, 0u64, 0u64);
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(d: *const u16, fc: *mut u64, tot: *mut u64, ft: *mut u64) -> i32;
        }
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut fc, &mut tot, &mut ft) };
        if ok != 0 { Some((tot - ft, ft, tot)) } else { None }
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = root; None }
}

fn apply_theme(ctx: &egui::Context) {
    let mut s = (*ctx.style()).clone();
    s.visuals.dark_mode                        = true;
    s.visuals.panel_fill                       = BG_DEEP;
    s.visuals.window_fill                      = BG_PANEL;
    s.visuals.extreme_bg_color                 = BG_DEEP;
    s.visuals.faint_bg_color                   = BG_HEADER;
    s.visuals.override_text_color              = Some(TEXT_PRIMARY);
    s.visuals.widgets.noninteractive.bg_fill   = BG_PANEL;
    s.visuals.widgets.inactive.bg_fill         = BG_PANEL;
    s.visuals.widgets.hovered.bg_fill          = BG_HOVER;
    s.visuals.widgets.active.bg_fill           = BG_SELECTED;
    s.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, BORDER);
    s.visuals.widgets.inactive.fg_stroke       = Stroke::new(1.0, BORDER);
    s.visuals.widgets.hovered.fg_stroke        = Stroke::new(1.0, ACCENT);
    s.visuals.selection.bg_fill                = BG_SELECTED;
    s.visuals.selection.stroke                 = Stroke::new(1.0, ACCENT);
    s.spacing.item_spacing                     = Vec2::new(8.0, 4.0);
    s.spacing.button_padding                   = Vec2::new(12.0, 6.0);
    ctx.set_style(s);
}
