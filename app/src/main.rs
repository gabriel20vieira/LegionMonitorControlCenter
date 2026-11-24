#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod edid;
mod overlay;
mod store;
mod update;
mod vcp;
mod win;
mod worker;

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, Vec2};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use vcp::{Caps, Kind, Tab};
use win::{ModeInfo, MonitorDesc};
use worker::{Cmd, Msg};

// Palette taken from the original "artery" (gaming) theme: static/style/artery/common.css
const BG: Color32 = Color32::from_rgb(0x1f, 0x2a, 0x42);
const PANEL: Color32 = Color32::from_rgb(0x2e, 0x3b, 0x56);
const CARD: Color32 = Color32::from_rgb(0x26, 0x33, 0x4d);
const ACCENT: Color32 = Color32::from_rgb(0x49, 0x90, 0xff);
const TEXT: Color32 = Color32::from_rgb(0xf1, 0xf1, 0xf1);
const MUTED: Color32 = Color32::from_rgb(0x8a, 0xb1, 0xed);
const WARN: Color32 = Color32::from_rgb(0xfd, 0x4c, 0x4c);
const PANEL_SOFT: Color32 = Color32::from_rgb(0x23, 0x2f, 0x4a);
const BORDER: Color32 = Color32::from_rgb(0x36, 0x45, 0x63);
const ACCENT_DIM: Color32 = Color32::from_rgb(0x37, 0x59, 0x94);
const ACCENT_SOFT: Color32 = Color32::from_rgb(0x2a, 0x45, 0x75);

const APP_NAME: &str = "Monitor Control Center";
/// How long a new resolution/rotation stays without confirmation before it is undone.
const KEEP_SECS: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Feature(Tab),
    Display,
}

/// A resolution/rotation change waiting for "Manter" (reverted when the time runs out).
struct KeepMode {
    key: String,
    gdi: String,
    number: u32,
    before: ModeInfo,
    what: String,
    deadline: Instant,
}

struct Shot {
    path: PathBuf,
    frames: u32,
    requested: bool,
}

struct App {
    tx: Sender<Cmd>,
    rx: Receiver<Msg>,
    monitors: Vec<MonitorDesc>,
    scanned: bool,
    selected: Option<String>,
    values: HashMap<String, HashMap<u8, Option<(u32, u32)>>>,
    caps: HashMap<String, Caps>,
    reading: HashSet<String>,
    modes: HashMap<String, ModeInfo>,
    pending_mode: Option<(String, (u32, u32), u32)>,
    store: store::Store,
    section: Section,
    status: String,
    status_bad: bool,
    nickname_edit: Option<String>,
    confirm_reset: bool,
    shot: Option<Shot>,
    logo: egui::TextureHandle,
    show_details: bool,
    flash_until: Option<Instant>,
    all_brightness: Option<u32>,
    profile_edit: Option<String>,
    active_profile: Option<String>,
    /// Next scan result should not overwrite the status line.
    keep_status: bool,
    scanned_once: bool,
    keep: Option<KeepMode>,
    updates: Option<Receiver<update::Event>>,
    update_ready: Option<(String, PathBuf)>,
    update_installed: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, shot: Option<PathBuf>) -> Self {
        style(&cc.egui_ctx);
        let (tx, rx) = worker::spawn(cc.egui_ctx.clone());
        let _ = tx.send(Cmd::Scan);
        let store = store::Store::load();
        let logo = {
            let img = image::load_from_memory(include_bytes!("../assets/icon-256.png")).expect("logo png").to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            cc.egui_ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), Default::default())
        };
        // No update checks in debug builds or when only taking a screenshot.
        let updates = (!cfg!(debug_assertions) && shot.is_none() && !std::env::args().any(|a| a == "--no-update"))
            .then(|| update::spawn(cc.egui_ctx.clone()));
        App {
            tx,
            rx,
            monitors: Vec::new(),
            scanned: false,
            selected: store.last_selected.clone(),
            values: HashMap::new(),
            caps: HashMap::new(),
            reading: HashSet::new(),
            modes: HashMap::new(),
            pending_mode: None,
            store,
            section: Section::Feature(Tab::Image),
            status: "A procurar monitores…".into(),
            status_bad: false,
            nickname_edit: None,
            confirm_reset: false,
            shot: shot.map(|path| Shot { path, frames: 0, requested: false }),
            logo,
            show_details: false,
            flash_until: None,
            all_brightness: None,
            profile_edit: None,
            active_profile: None,
            keep_status: false,
            scanned_once: false,
            keep: None,
            updates,
            update_ready: None,
            update_installed: false,
        }
    }

    fn monitor(&self, key: &str) -> Option<&MonitorDesc> {
        self.monitors.iter().find(|m| m.key == key)
    }

    fn label(&self, m: &MonitorDesc) -> String {
        match self.store.nicknames.get(&m.key) {
            Some(n) if !n.trim().is_empty() => format!("{} · {}", m.win_number, n.trim()),
            _ => m.title(),
        }
    }

    /// Monitors that share model with `m` (the case the original got wrong).
    fn twins(&self, m: &MonitorDesc) -> usize {
        self.monitors
            .iter()
            .filter(|o| o.key != m.key && o.name == m.name && o.hardware_id == m.hardware_id)
            .count()
    }

    fn select(&mut self, key: String) {
        if self.selected.as_deref() != Some(key.as_str()) || !self.values.contains_key(&key) {
            self.request_read(&key);
        }
        if let Some(m) = self.monitor(&key) {
            let gdi = m.gdi_name.clone();
            self.modes.insert(key.clone(), win::mode_info(&gdi));
        }
        // The selected monitor is the source; it cannot also follow.
        self.store.sync.remove(&key);
        self.selected = Some(key.clone());
        self.store.last_selected = Some(key);
        self.store.save();
        self.nickname_edit = None;
        self.confirm_reset = false;
        self.pending_mode = None;
        self.profile_edit = None;
        self.active_profile = None;
    }

    fn request_read(&mut self, key: &str) {
        let has_ddc = self.monitor(key).map(|m| m.ddc).unwrap_or(false);
        if !has_ddc || self.reading.contains(key) {
            return;
        }
        self.reading.insert(key.to_string());
        let _ = self.tx.send(Cmd::Read(key.to_string()));
        if !self.caps.contains_key(key) {
            let _ = self.tx.send(Cmd::Caps(key.to_string()));
        }
    }

    /// Selected monitor plus the ones the person ticked to follow it.
    fn targets(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Some(sel) = &self.selected {
            v.push(sel.clone());
            for m in &self.monitors {
                if &m.key != sel && m.ddc && self.store.sync.contains(&m.key) {
                    v.push(m.key.clone());
                }
            }
        }
        v
    }

    fn set_vcp(&mut self, code: u8, value: u32) {
        let targets = self.targets();
        for k in &targets {
            if let Some(map) = self.values.get_mut(k) {
                let max = map.get(&code).copied().flatten().map(|v| v.1).unwrap_or(100);
                map.insert(code, Some((value, max)));
            }
        }
        let _ = self.tx.send(Cmd::Set(targets, code, value));
        self.flash_until = Some(Instant::now() + Duration::from_millis(600));
        if code == vcp::BRIGHTNESS {
            self.all_brightness = None;
        }
    }

    fn pump(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Monitors(list) => {
                    self.scanned = true;
                    let keys: HashSet<String> = list.iter().map(|m| m.key.clone()).collect();
                    self.values.retain(|k, _| keys.contains(k));
                    self.monitors = list;
                    let n = self.monitors.len();
                    let off = self.monitors.iter().filter(|m| m.off).count();
                    let old_status = std::mem::take(&mut self.keep_status).then(|| (self.status.clone(), self.status_bad));
                    self.status = match n {
                        0 => "Nenhum monitor encontrado.".into(),
                        1 => "1 monitor encontrado.".into(),
                        _ => format!("{n} monitores encontrados, cada um com o seu número do Windows e ID único."),
                    };
                    if off > 0 {
                        self.status += &format!(" {off} desligado(s).");
                    }
                    self.status_bad = n == 0;
                    if let Some((t, bad)) = old_status {
                        self.status = t;
                        self.status_bad = bad;
                    }
                    // Keep the selection, unless it is a screen that was already off at start.
                    let keep = self.selected.clone().filter(|k| keys.contains(k)).filter(|k| {
                        self.scanned_once || self.monitors.iter().any(|m| &m.key == k && !m.off)
                    });
                    self.scanned_once = true;
                    let pick = keep
                        .or_else(|| self.monitors.iter().find(|m| m.primary).map(|m| m.key.clone()))
                        .or_else(|| self.monitors.first().map(|m| m.key.clone()));
                    self.reading.clear();
                    if let Some(k) = pick {
                        self.selected = None;
                        self.select(k);
                    }
                }
                Msg::Value(key, code, v) => {
                    self.values.entry(key).or_default().insert(code, v);
                }
                Msg::ReadDone(key) => {
                    self.reading.remove(&key);
                }
                Msg::Caps(key, raw) => {
                    let caps = raw.map(|r| vcp::parse_caps(&r)).unwrap_or_default();
                    self.caps.insert(key, caps);
                }
                Msg::SetDone { code, value, ok, failed } => {
                    let name = vcp::FEATURES.iter().find(|f| f.code == code).map(|f| f.name).unwrap_or("Definição");
                    let names = |keys: &[String]| {
                        keys.iter()
                            .filter_map(|k| self.monitor(k))
                            .map(|m| format!("ecrã {}", m.win_number))
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    if failed.is_empty() {
                        self.status = format!("{name} = {value} aplicado a: {}.", names(&ok));
                        self.status_bad = false;
                    } else {
                        self.status = format!("{name}: o monitor não aceitou em {}.", names(&failed));
                        self.status_bad = true;
                    }
                }
            }
        }
    }
}

fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(0x17, 0x20, 0x33);
    v.faint_bg_color = CARD;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0_f32, TEXT);
    v.hyperlink_color = ACCENT;
    v.widgets.inactive.bg_fill = PANEL;
    v.widgets.inactive.weak_bg_fill = PANEL;
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x37, 0x59, 0x94);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x37, 0x59, 0x94);
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = ACCENT;
    v.slider_trailing_fill = true;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.slider_width = 320.0;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Heading, FontId::proportional(22.0));
    });
}

fn badge(ui: &mut egui::Ui, number: u32, size: f32, fill: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, number.to_string(), FontId::proportional(size * 0.55), Color32::WHITE);
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();
        self.pump_updates();
        self.screenshot_step(ctx);
        if self.flash_until.is_some_and(|t| t > Instant::now()) {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        self.top_bar(ctx);
        self.update_banner(ctx);

        egui::TopBottomPanel::bottom("status")
            .exact_height(30.0)
            .frame(egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::symmetric(16, 6)))
            .show(ctx, |ui| {
                ui.label(RichText::new(&self.status).color(if self.status_bad { WARN } else { MUTED }));
            });

        egui::CentralPanel::default().frame(egui::Frame::default().fill(BG)).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                self.screens_section(ui);
                egui::Frame::default().inner_margin(egui::Margin::symmetric(24, 18)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.monitor_panel(ui);
                });
            });
        });

        self.modals(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // An update downloaded but not installed yet goes in now, ready for the next start.
        if !self.update_installed {
            if let Some((_, staged)) = &self.update_ready {
                let _ = update::install(staged);
            }
        }
    }
}

/// A switch with a label, drawn as one clickable chip.
fn switch_chip(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(14.0), TEXT);
    let size = Vec2::new(galley.size().x + 58.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let alpha = if enabled { 1.0 } else { 0.4 };
    let p = ui.painter();
    let border = if on { ACCENT } else if resp.hovered() && enabled { ACCENT_DIM } else { BORDER };
    p.rect_filled(rect, 6.0, if on { ACCENT_SOFT } else { CARD }.gamma_multiply(alpha));
    p.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, border.gamma_multiply(alpha)), egui::StrokeKind::Inside);
    let track = egui::Rect::from_min_size(rect.left_center() + Vec2::new(10.0, -8.0), Vec2::new(30.0, 16.0));
    p.rect_filled(track, 8.0, if on { ACCENT } else { Color32::from_rgb(0x55, 0x63, 0x80) }.gamma_multiply(alpha));
    let knob_x = if on { track.right() - 8.0 } else { track.left() + 8.0 };
    p.circle_filled(egui::pos2(knob_x, track.center().y), 6.0, Color32::WHITE.gamma_multiply(alpha));
    p.galley(egui::pos2(track.right() + 8.0, rect.center().y - galley.size().y / 2.0), galley, TEXT.gamma_multiply(alpha));
    if enabled {
        resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    resp
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(MUTED).size(13.0).strong());
}

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::default()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

impl App {
    fn current(&self) -> Option<MonitorDesc> {
        self.selected.as_deref().and_then(|k| self.monitor(k)).cloned()
    }

    fn display_name(&self, m: &MonitorDesc) -> String {
        let lbl = self.label(m);
        lbl.split_once(" · ").map(|x| x.1.to_string()).unwrap_or(lbl)
    }

    fn rescan(&mut self) {
        self.status = "A procurar monitores…".into();
        self.status_bad = false;
        self.scanned = false;
        let _ = self.tx.send(Cmd::Scan);
    }

    fn identify(&self) {
        let items = self
            .monitors
            .iter()
            .filter(|m| m.sub_index == 0 && !m.off)
            .map(|m| {
                let cap = format!("{}\n{}", self.display_name(m), m.instance_short);
                (m.rect, m.win_number.to_string(), cap, Some(&m.key) == self.selected.as_ref())
            })
            .collect();
        overlay::identify(items, 2500);
    }

    fn pump_updates(&mut self) {
        let Some(rx) = &self.updates else { return };
        while let Ok(ev) = rx.try_recv() {
            match ev {
                update::Event::Ready { version, staged } => self.update_ready = Some((version, staged)),
            }
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .exact_height(56.0)
            .frame(egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::symmetric(16, 8)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add(egui::Image::new((self.logo.id(), Vec2::splat(30.0))).corner_radius(6.0));
                    ui.add_space(4.0);
                    ui.label(RichText::new(APP_NAME).size(19.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("⟳  Procurar monitores").clicked() {
                            self.rescan();
                        }
                        if ui.button("◎  Identificar").on_hover_text("Mostra o número do Windows em cada ecrã").clicked() {
                            self.identify();
                        }
                    });
                });
            });
    }

    fn update_banner(&mut self, ctx: &egui::Context) {
        let Some((version, staged)) = self.update_ready.clone() else { return };
        if self.update_installed {
            return;
        }
        egui::TopBottomPanel::top("update")
            .frame(egui::Frame::default().fill(ACCENT_SOFT).inner_margin(egui::Margin::symmetric(16, 8)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Nova versão {version} pronta a instalar.")).strong());
                    ui.label(RichText::new(format!("(tem a {})", update::current_version())).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Reiniciar e atualizar").clicked() {
                            match update::install(&staged) {
                                Ok(exe) => {
                                    self.update_installed = true;
                                    match std::process::Command::new(&exe).spawn() {
                                        Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                                        Err(e) => {
                                            self.status = format!("Atualizado para {version}, mas não reabriu ({e}). Abra a app de novo.");
                                            self.status_bad = true;
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.status = format!("Não foi possível instalar a atualização: {e}");
                                    self.status_bad = true;
                                }
                            }
                        }
                        ui.label(RichText::new("ou é instalada quando fechar a app").color(MUTED));
                    });
                });
            });
    }

    // ---------- "Os seus ecrãs": map, follow switches, brightness of all ----------

    fn screens_section(&mut self, ui: &mut egui::Ui) {
        egui::Frame::default().fill(PANEL_SOFT).inner_margin(egui::Margin::symmetric(24, 16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                section_title(ui, "OS SEUS ECRÃS");
                if !self.scanned {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Clique num ecrã para o escolher").size(12.0).color(MUTED));
                });
            });
            ui.add_space(8.0);
            self.display_map(ui);
            ui.add_space(12.0);
            self.power_row(ui);
            ui.add_space(6.0);
            self.follow_row(ui);
            self.all_brightness_row(ui);
        });
    }

    fn display_map(&mut self, ui: &mut egui::Ui) {
        if self.monitors.is_empty() {
            ui.label(RichText::new(if self.scanned { "Nenhum monitor encontrado." } else { "A procurar monitores…" }).color(MUTED));
            return;
        }
        let on: Vec<MonitorDesc> = self.monitors.iter().filter(|m| !m.off).cloned().collect();
        if on.is_empty() {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for m in &on {
            x0 = x0.min(m.rect.left);
            y0 = y0.min(m.rect.top);
            x1 = x1.max(m.rect.right);
            y1 = y1.max(m.rect.bottom);
        }
        let (bw, bh) = ((x1 - x0).max(1) as f32, (y1 - y0).max(1) as f32);
        let avail = ui.available_width();
        let scale = (150.0 / bh).min(avail / bw);
        let size = Vec2::new(bw * scale, bh * scale);
        let (area, _) = ui.allocate_exact_size(Vec2::new(avail, size.y), Sense::hover());
        let origin = egui::pos2(area.center().x - size.x / 2.0, area.top());

        let cur_ddc = self.current().is_some_and(|m| m.ddc);
        let mut clicked = None;
        for m in &on {
            // Physical monitors cloned on the same output share a rect: nudge them apart.
            let nudge = m.sub_index as f32 * 8.0;
            let r = egui::Rect::from_min_size(
                origin + Vec2::new((m.rect.left - x0) as f32 * scale + nudge, (m.rect.top - y0) as f32 * scale + nudge),
                Vec2::new(m.width() as f32 * scale, m.height() as f32 * scale),
            )
            .shrink(3.0);
            let name = self.display_name(m);
            let resp = ui
                .interact(r, ui.id().with(("map", &m.key)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(format!("Ecrã {} · {} · S/N {}", m.win_number, name, m.serial_display()));
            let sel = self.selected.as_deref() == Some(m.key.as_str());
            let fol = !sel && cur_ddc && m.ddc && self.store.sync.contains(&m.key);
            let (fill, stroke, text) = if sel {
                (ACCENT, ACCENT, Color32::WHITE)
            } else if fol {
                (ACCENT_SOFT, ACCENT, TEXT)
            } else {
                (CARD, if resp.hovered() { ACCENT_DIM } else { BORDER }, TEXT)
            };
            let p = ui.painter().with_clip_rect(r.expand(1.0));
            p.rect_filled(r, 6.0, fill);
            p.rect_stroke(r, 6.0, Stroke::new(2.0_f32, stroke), egui::StrokeKind::Inside);
            let big = (r.height() * 0.32).clamp(14.0, 28.0);
            p.text(r.center() - Vec2::new(0.0, big * 0.35), Align2::CENTER_CENTER, m.win_number.to_string(), FontId::proportional(big), text);
            if r.height() > 60.0 {
                p.text(r.center() + Vec2::new(0.0, big * 0.55), Align2::CENTER_CENTER, &name, FontId::proportional(11.5), text.gamma_multiply(0.85));
            }
            if fol {
                let tag = egui::Rect::from_min_size(r.right_top() + Vec2::new(-58.0, 5.0), Vec2::new(53.0, 17.0));
                p.rect_filled(tag, 4.0, ACCENT);
                p.text(tag.center(), Align2::CENTER_CENTER, "🔗 segue", FontId::proportional(11.0), Color32::WHITE);
            } else if !m.ddc && r.height() > 60.0 {
                p.text(r.center_bottom() - Vec2::new(0.0, 9.0), Align2::CENTER_CENTER, "sem DDC/CI", FontId::proportional(10.5), WARN);
            }
            if resp.clicked() {
                clicked = Some(m.key.clone());
            }
        }
        if let Some(k) = clicked {
            self.select(k);
        }
    }

    fn follow_row(&mut self, ui: &mut egui::Ui) {
        let Some(cur) = self.current() else { return };
        if cur.off {
            return;
        }
        if !cur.ddc {
            ui.label(RichText::new(format!("O ecrã {} não tem DDC/CI, por isso não há alterações para seguir.", cur.win_number)).color(MUTED));
            return;
        }
        let others: Vec<(String, u32, bool)> =
            self.monitors.iter().filter(|m| m.key != cur.key && !m.off).map(|m| (m.key.clone(), m.win_number, m.ddc)).collect();
        if others.is_empty() {
            return;
        }
        let mut toggled = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("Seguem as alterações do ecrã {}:", cur.win_number)).color(MUTED));
            for (key, n, ddc) in &others {
                let on = *ddc && self.store.sync.contains(key);
                let text = if *ddc { format!("🔗 Ecrã {n}") } else { format!("Ecrã {n} (sem DDC/CI)") };
                let resp = ui.add_enabled_ui(*ddc, |ui| switch_chip(ui, on, &text)).inner;
                if resp.clicked() {
                    toggled = Some((key.clone(), *n));
                }
            }
        });
        if let Some((k, n)) = toggled {
            if self.store.sync.remove(&k) {
                self.status = format!("O ecrã {n} deixou de seguir o ecrã {}.", cur.win_number);
            } else {
                self.store.sync.insert(k);
                self.status = format!("O ecrã {n} passa a receber as alterações do ecrã {}.", cur.win_number);
            }
            self.status_bad = false;
            self.store.save();
        }
    }

    /// "Ecrãs ligados: [5] [6] [7]" - turns whole screens on and off (one switch per Windows display).
    fn power_row(&mut self, ui: &mut egui::Ui) {
        let screens: Vec<(String, u32, bool, bool)> = self
            .monitors
            .iter()
            .filter(|m| m.sub_index == 0)
            .map(|m| (m.key.clone(), m.win_number, !m.off, m.primary))
            .collect();
        if screens.len() < 2 {
            return;
        }
        let mut toggled = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Ecrãs ligados:").color(MUTED));
            for (key, n, on, primary) in &screens {
                let text = format!("Ecrã {n}");
                let resp = ui.add_enabled_ui(!*primary, |ui| switch_chip(ui, *on, &text)).inner;
                let resp = if *primary {
                    resp.on_disabled_hover_text("É o ecrã principal do Windows: não pode ser desligado aqui.")
                } else if *on {
                    resp.on_hover_text("Desligar: sai do ambiente de trabalho e o monitor deixa de receber imagem")
                } else {
                    resp.on_hover_text("Ligar: volta ao ambiente de trabalho no sítio onde estava")
                };
                if resp.clicked() {
                    toggled = Some((key.clone(), !*on));
                }
            }
        });
        if let Some(t) = toggled {
            self.set_screens(vec![t], &HashMap::new());
            self.active_profile = None;
        }
    }

    /// Which screens are on now, and where, for saving in a profile.
    fn current_screens(&self) -> Vec<store::LayoutScreen> {
        self.monitors
            .iter()
            .filter(|m| m.sub_index == 0)
            .map(|m| store::LayoutScreen {
                key: m.key.clone(),
                gdi: m.gdi_name.clone(),
                on: !m.off,
                place: if m.off { self.store.placements.get(&m.key).copied() } else { win::placement(&m.gdi_name) },
            })
            .collect()
    }

    /// Turns screens on/off as saved in a profile. Returns true when nothing failed.
    fn apply_screens(&mut self, screens: &[store::LayoutScreen]) -> bool {
        let mut want = Vec::new();
        let mut places = HashMap::new();
        for s in screens {
            let m = self
                .monitors
                .iter()
                .find(|m| m.sub_index == 0 && m.key == s.key)
                .or_else(|| self.monitors.iter().find(|m| m.sub_index == 0 && m.gdi_name == s.gdi));
            if let Some(m) = m {
                want.push((m.key.clone(), s.on));
                if let Some(p) = s.place {
                    places.insert(m.key.clone(), p);
                }
            }
        }
        let already = want.iter().all(|(k, on)| self.monitor(k).is_some_and(|m| m.off != *on));
        already || self.set_screens(want, &places)
    }

    fn screen_number(&self, s: &store::LayoutScreen) -> u32 {
        self.monitors
            .iter()
            .find(|m| m.key == s.key)
            .map(|m| m.win_number)
            .unwrap_or_else(|| s.gdi.trim_start_matches(|c: char| !c.is_ascii_digit()).parse().unwrap_or(0))
    }

    /// Turns screens (by unique key) on or off. `places` overrides where a screen comes back.
    /// Returns true when everything was applied.
    fn set_screens(&mut self, want: Vec<(String, bool)>, places: &HashMap<String, win::Placement>) -> bool {
        let mut changes = Vec::new();
        let mut on_now: Vec<String> = Vec::new();
        let mut off_now: Vec<String> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        let mut stay_on: HashSet<String> =
            self.monitors.iter().filter(|m| m.sub_index == 0 && !m.off).map(|m| m.gdi_name.clone()).collect();
        for (key, on) in &want {
            let Some(m) = self.monitor(key).cloned() else { continue };
            if *on && m.off {
                let at = places.get(key).copied().or_else(|| self.store.placements.get(key).copied());
                changes.push(win::Power::On(key.clone(), at));
                on_now.push(m.win_number.to_string());
            } else if !*on && !m.off {
                if m.primary {
                    skipped.push(format!("o ecrã {} é o principal do Windows", m.win_number));
                    continue;
                }
                if let Some(p) = win::placement(&m.gdi_name) {
                    self.store.placements.insert(key.clone(), p);
                }
                stay_on.remove(&m.gdi_name);
                changes.push(win::Power::Off(key.clone()));
                off_now.push(m.win_number.to_string());
            }
        }
        if stay_on.is_empty() && on_now.is_empty() {
            self.status = "Tem de ficar pelo menos um ecrã ligado.".into();
            self.status_bad = true;
            return false;
        }
        if changes.is_empty() {
            self.status = if skipped.is_empty() { "Os ecrãs já estão assim.".into() } else { format!("Nada mudou: {}.", skipped.join("; ")) };
            self.status_bad = !skipped.is_empty();
            return skipped.is_empty();
        }
        self.store.save();
        let failed = win::set_power(&changes);
        let mut parts = Vec::new();
        if !off_now.is_empty() {
            parts.push(format!("desligado(s): ecrã {}", off_now.join(", ")));
        }
        if !on_now.is_empty() {
            parts.push(format!("ligado(s): ecrã {}", on_now.join(", ")));
        }
        let ok = failed.is_empty() && skipped.is_empty();
        self.status = if failed.is_empty() {
            let mut t = format!("Ecrãs {}.", parts.join("; "));
            if !skipped.is_empty() {
                t += &format!(" Não desliguei: {}.", skipped.join("; "));
            }
            t
        } else {
            let why: Vec<String> = failed
                .iter()
                .map(|(k, e)| match self.monitor(k) {
                    Some(m) => format!("ecrã {}: {e}", m.win_number),
                    None => e.clone(),
                })
                .collect();
            format!("O Windows não aplicou tudo ({}).", why.join("; "))
        };
        self.status_bad = !ok;
        self.rescan_quiet();
        ok
    }

    /// Scan again but keep the current status line.
    fn rescan_quiet(&mut self) {
        self.scanned = false;
        self.keep_status = true;
        let _ = self.tx.send(Cmd::Scan);
    }

    fn all_brightness_row(&mut self, ui: &mut egui::Ui) {
        let ddc: Vec<String> = self.monitors.iter().filter(|m| m.ddc).map(|m| m.key.clone()).collect();
        if ddc.len() < 2 {
            return;
        }
        let known: Vec<u32> = ddc
            .iter()
            .filter_map(|k| self.values.get(k)?.get(&vcp::BRIGHTNESS).copied().flatten())
            .map(|(cur, max)| cur * 100 / max.max(1))
            .collect();
        let mut v = self.all_brightness.unwrap_or_else(|| if known.is_empty() { 50 } else { known.iter().sum::<u32>() / known.len() as u32 });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("☀").size(16.0).color(MUTED));
                ui.add_sized([150.0, 22.0], egui::Label::new(RichText::new("Brilho de todos").strong()));
                ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(120.0);
                if ui.add(egui::Slider::new(&mut v, 0..=100)).changed() {
                    self.all_brightness = Some(v);
                    for k in &ddc {
                        let max = self.values.get(k).and_then(|m| m.get(&vcp::BRIGHTNESS).copied().flatten()).map(|x| x.1).unwrap_or(100);
                        let scaled = v * max / 100;
                        if let Some(map) = self.values.get_mut(k) {
                            map.insert(vcp::BRIGHTNESS, Some((scaled, max)));
                        }
                        let _ = self.tx.send(Cmd::Set(vec![k.clone()], vcp::BRIGHTNESS, scaled));
                    }
                }
            });
        });
    }

    // ---------- Selected monitor ----------

    fn monitor_panel(&mut self, ui: &mut egui::Ui) {
        let Some(m) = self.current() else {
            ui.label(RichText::new("Escolha um ecrã no mapa acima.").color(MUTED));
            return;
        };
        self.header(ui, &m);
        if m.off {
            ui.add_space(10.0);
            card(ui, |ui| {
                ui.label(RichText::new("Este ecrã está desligado").strong());
                ui.label(
                    RichText::new("Está ligado ao computador, mas fora do ambiente de trabalho do Windows, por isso o monitor não recebe imagem.")
                        .color(MUTED),
                );
                ui.add_space(4.0);
                if ui.button(format!("Ligar ecrã {}", m.win_number)).clicked() {
                    self.set_screens(vec![(m.key.clone(), true)], &HashMap::new());
                }
            });
            return;
        }

        if self.twins(&m) > 0 {
            ui.add_space(8.0);
            egui::Frame::default().fill(Color32::from_rgb(0x3a, 0x34, 0x22)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let other = self
                    .monitors
                    .iter()
                    .filter(|o| o.key != m.key && o.name == m.name && o.hardware_id == m.hardware_id)
                    .map(|o| o.win_number.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                ui.label(RichText::new(format!(
                    "O ecrã {other} é do mesmo modelo. As alterações aqui vão só para este (S/N {}), a não ser que o ligue em \"Seguir\".",
                    m.serial_display()
                ))
                .color(Color32::from_rgb(0xff, 0xd9, 0x8a)));
            });
        }

        ui.add_space(10.0);
        if m.ddc {
            self.destination(ui, &m);
            ui.add_space(10.0);
            card(ui, |ui| {
                for code in [vcp::BRIGHTNESS, vcp::CONTRAST] {
                    let f = vcp::FEATURES.iter().find(|f| f.code == code).unwrap();
                    self.feature_row(ui, &m, f);
                }
            });
            ui.add_space(10.0);
            self.profiles_row(ui, &m);
        } else {
            card(ui, |ui| {
                ui.label(RichText::new("Este ecrã não aceita controlo DDC/CI").strong().color(WARN));
                ui.label(
                    RichText::new(
                        "Ecrãs internos de portátil não usam DDC/CI: o brilho deles é controlado pelo Windows. \
                         Num monitor externo, ative \"DDC/CI\" no menu do próprio monitor e procure de novo.",
                    )
                    .color(MUTED),
                );
                if ui.button("⟳  Procurar de novo").clicked() {
                    self.rescan();
                }
            });
        }

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            for (s, t) in [
                (Section::Feature(Tab::Image), "Imagem"),
                (Section::Feature(Tab::Game), "Jogo"),
                (Section::Feature(Tab::Input), "Entrada e energia"),
                (Section::Display, "Ecrã"),
            ] {
                if ui.selectable_label(self.section == s, RichText::new(t).size(15.0)).clicked() {
                    self.section = s;
                }
            }
        });
        ui.separator();
        ui.add_space(4.0);
        match self.section {
            Section::Feature(tab) => self.features(ui, &m, tab),
            Section::Display => self.display(ui, &m),
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, m: &MonitorDesc) {
        let key = m.key.clone();
        let mode = self.modes.get(&key).cloned().unwrap_or_default();
        ui.horizontal(|ui| {
            badge(ui, m.win_number, 56.0, ACCENT);
            ui.vertical(|ui| {
                match &mut self.nickname_edit {
                    Some(text) => {
                        let mut done = false;
                        ui.horizontal(|ui| {
                            let r = ui.add(egui::TextEdit::singleline(text).hint_text(m.name.as_str()).desired_width(260.0));
                            if ui.button("Guardar").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                                done = true;
                            }
                        });
                        if done {
                            let t = text.trim().to_string();
                            if t.is_empty() {
                                self.store.nicknames.remove(&key);
                            } else {
                                self.store.nicknames.insert(key.clone(), t);
                            }
                            self.store.save();
                            self.nickname_edit = None;
                        }
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.heading(self.display_name(m));
                            if ui.small_button("✏ Nome").on_hover_text("Dê um nome próprio a este ecrã").clicked() {
                                self.nickname_edit = Some(self.store.nicknames.get(&key).cloned().unwrap_or_default());
                            }
                        });
                    }
                }
                let mut line = format!("Ecrã {} do Windows  ·  S/N {}", m.win_number, m.serial_display());
                if mode.width > 0 {
                    line += &format!("  ·  {} x {}", mode.width, mode.height);
                }
                if mode.freq > 1 {
                    line += &format!("  ·  {} Hz", mode.freq);
                }
                ui.label(RichText::new(line).color(MUTED));
                let t = if self.show_details { "⏶ Esconder detalhes" } else { "⏷ Detalhes técnicos" };
                if ui.add(egui::Label::new(RichText::new(t).size(13.0).color(ACCENT)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.show_details = !self.show_details;
                }
            });
        });
        if self.show_details {
            ui.add_space(8.0);
            card(ui, |ui| {
                egui::Grid::new("info").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                    let row = |ui: &mut egui::Ui, k: &str, v: String| {
                        ui.label(RichText::new(k).color(MUTED));
                        ui.label(v);
                        ui.end_row();
                    };
                    row(ui, "Modelo", m.name.clone());
                    row(ui, "Fabricante", if m.manufacturer.is_empty() { "—".into() } else { m.manufacturer.clone() });
                    row(ui, "Número de série", m.serial_display());
                    row(ui, "Nome no Windows", m.gdi_name.clone());
                    row(ui, "ID único do dispositivo", if m.instance_path.is_empty() { "—".into() } else { m.instance_path.clone() });
                    row(ui, "Posição no ambiente de trabalho", format!("{}, {}  ({} x {})", m.rect.left, m.rect.top, m.width(), m.height()));
                    row(ui, "Ano de fabrico", if m.year > 1990 { m.year.to_string() } else { "—".into() });
                    row(ui, "Controlo DDC/CI", if m.ddc { "disponível".into() } else { "indisponível".into() });
                });
            });
        }
    }

    /// "As alterações vão para: 2 + 3" — lights up briefly after each change.
    fn destination(&mut self, ui: &mut egui::Ui, m: &MonitorDesc) {
        let lit = self.flash_until.is_some_and(|t| t > Instant::now());
        let followers: Vec<u32> = self
            .monitors
            .iter()
            .filter(|o| o.key != m.key && o.ddc && self.store.sync.contains(&o.key))
            .map(|o| o.win_number)
            .collect();
        egui::Frame::default()
            .fill(if lit { ACCENT_SOFT } else { BG })
            .stroke(Stroke::new(1.0_f32, if lit { ACCENT } else { BORDER }))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⏵  As alterações vão para:").color(MUTED));
                    badge(ui, m.win_number, 24.0, ACCENT);
                    for n in &followers {
                        ui.label(RichText::new("+").color(MUTED));
                        badge(ui, *n, 24.0, PANEL);
                    }
                    if followers.is_empty() {
                        ui.label(RichText::new("(só este ecrã)").size(13.0).color(MUTED));
                    }
                });
            });
    }

    fn profiles_row(&mut self, ui: &mut egui::Ui, m: &MonitorDesc) {
        let mut apply = None;
        let mut delete = None;
        let mut save = None;
        ui.horizontal_wrapped(|ui| {
            section_title(ui, "PERFIS");
            ui.add_space(4.0);
            for (i, p) in self.store.profiles.iter().enumerate() {
                let on: Vec<String> = p.screens.iter().filter(|s| s.on).map(|s| self.screen_number(s).to_string()).collect();
                let hint = if on.is_empty() {
                    "Clique para aplicar · botão direito para apagar".to_string()
                } else {
                    format!("Ecrãs ligados: {}\nClique para aplicar · botão direito para apagar", on.join(", "))
                };
                let r = ui
                    .add(egui::SelectableLabel::new(self.active_profile.as_deref() == Some(p.name.as_str()), &p.name))
                    .on_hover_text(hint);
                if r.clicked() {
                    apply = Some(i);
                }
                r.context_menu(|ui| {
                    if ui.button("Apagar perfil").clicked() {
                        delete = Some(i);
                        ui.close_menu();
                    }
                });
            }
            match &mut self.profile_edit {
                Some(name) => {
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width(160.0));
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("Guardar").clicked() || enter {
                        save = Some(name.trim().to_string());
                    }
                    if ui.button("Cancelar").clicked() {
                        self.profile_edit = None;
                    }
                }
                None => {
                    if ui
                        .button("★ Guardar atual")
                        .on_hover_text("Guarda que ecrãs estão ligados e as definições de imagem e jogo deste ecrã")
                        .clicked()
                    {
                        self.profile_edit = Some(format!("Perfil {}", self.store.profiles.len() + 1));
                    }
                }
            }
        });
        if let Some(i) = apply {
            let p = self.store.profiles[i].clone();
            let screens_ok = self.apply_screens(&p.screens);
            let screens_status = (self.status.clone(), self.status_bad);
            for (code, value) in &p.values {
                self.set_vcp(*code, *value);
            }
            self.active_profile = Some(p.name.clone());
            let mut n = vec![m.win_number.to_string()];
            n.extend(self.monitors.iter().filter(|o| o.key != m.key && o.ddc && self.store.sync.contains(&o.key)).map(|o| o.win_number.to_string()));
            let on: Vec<String> = p.screens.iter().filter(|s| s.on).map(|s| self.screen_number(s).to_string()).collect();
            if !screens_ok {
                (self.status, self.status_bad) = screens_status;
            } else {
                let mut t = format!("Perfil {} aplicado", p.name);
                if !on.is_empty() {
                    t += &format!(": ecrãs ligados {}", on.join(", "));
                }
                if !p.values.is_empty() {
                    t += &format!("; imagem no ecrã {}", n.join(" e "));
                }
                self.status = t + ".";
                self.status_bad = false;
            }
        }
        if let Some(i) = delete {
            let p = self.store.profiles.remove(i);
            if self.active_profile.as_deref() == Some(p.name.as_str()) {
                self.active_profile = None;
            }
            self.store.save();
            self.status = format!("Perfil {} apagado.", p.name);
            self.status_bad = false;
        }
        if let Some(name) = save.filter(|n| !n.is_empty()) {
            // Image and game settings only: input source and power would cut the picture.
            let values: BTreeMap<u8, u32> = vcp::FEATURES
                .iter()
                .filter(|f| f.tab != Tab::Input)
                .filter_map(|f| {
                    let (cur, _) = self.values.get(&m.key)?.get(&f.code).copied().flatten()?;
                    Some((f.code, cur))
                })
                .collect();
            let screens = self.current_screens();
            if values.is_empty() && screens.len() < 2 {
                self.status = "Ainda não há valores lidos deste ecrã para guardar.".into();
                self.status_bad = true;
            } else {
                let on: Vec<String> = screens.iter().filter(|s| s.on).map(|s| self.screen_number(s).to_string()).collect();
                self.store.profiles.retain(|p| p.name != name);
                self.store.profiles.push(store::Profile { name: name.clone(), values, screens });
                self.store.save();
                self.active_profile = Some(name.clone());
                self.status = format!("Perfil {name} guardado: ecrãs ligados {} e definições do ecrã {}.", on.join(", "), m.win_number);
                self.status_bad = false;
            }
            self.profile_edit = None;
        }
    }

    fn features(&mut self, ui: &mut egui::Ui, m: &MonitorDesc, tab: Tab) {
        if !m.ddc {
            ui.label(RichText::new("Sem DDC/CI neste ecrã, por isso não há definições aqui. A resolução e a rotação estão em \"Ecrã\".").color(MUTED));
            return;
        }
        if self.reading.contains(&m.key) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("A ler definições do monitor…").color(MUTED));
            });
        }
        let caps = self.caps.get(&m.key).cloned().unwrap_or_default();
        card(ui, |ui| {
            let mut shown = 0;
            for f in vcp::FEATURES.iter().filter(|f| f.tab == tab) {
                if matches!(f.code, vcp::BRIGHTNESS | vcp::CONTRAST) {
                    continue;
                }
                let read = self.values.get(&m.key).and_then(|v| v.get(&f.code)).copied();
                let unsupported = matches!(read, Some(None)) && !caps.vcp.contains_key(&f.code) || !caps.supports(f.code);
                if unsupported {
                    continue;
                }
                self.feature_row(ui, m, f);
                shown += 1;
            }
            if shown == 0 && !self.reading.contains(&m.key) {
                ui.label(RichText::new("Este monitor não indica suporte para estas definições.").color(MUTED));
            }
        });
        if tab == Tab::Input {
            ui.add_space(10.0);
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Repor definições de fábrica").strong());
                        ui.label(RichText::new(format!("Só no ecrã {}.", m.win_number)).color(MUTED));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("↺  Repor de fábrica…").color(WARN)).clicked() {
                            self.confirm_reset = true;
                        }
                    });
                });
            });
        }
    }

    fn display(&mut self, ui: &mut egui::Ui, m: &MonitorDesc) {
        let mode = self.modes.get(&m.key).cloned().unwrap_or_default();
        card(ui, |ui| {
            if mode.modes.is_empty() {
                ui.label(RichText::new("O Windows não devolveu modos para este ecrã.").color(MUTED));
                return;
            }
            let (mut res, mut freq) = match &self.pending_mode {
                Some((k, r, f)) if k == &m.key => (*r, *f),
                _ => ((mode.width, mode.height), mode.freq),
            };
            ui.horizontal(|ui| {
                ui.add_sized([170.0, 24.0], egui::Label::new("Resolução"));
                egui::ComboBox::from_id_salt(("res", &m.key)).width(200.0).selected_text(format!("{} x {}", res.0, res.1)).show_ui(ui, |ui| {
                    for (r, rates) in &mode.modes {
                        if ui.selectable_label(*r == res, format!("{} x {}", r.0, r.1)).clicked() {
                            res = *r;
                            if !rates.contains(&freq) {
                                freq = rates.first().copied().unwrap_or(60);
                            }
                        }
                    }
                });
            });
            let rates = mode.modes.iter().find(|(r, _)| *r == res).map(|x| x.1.clone()).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.add_sized([170.0, 24.0], egui::Label::new("Frequência"));
                egui::ComboBox::from_id_salt(("hz", &m.key)).width(200.0).selected_text(format!("{freq} Hz")).show_ui(ui, |ui| {
                    for r in &rates {
                        if ui.selectable_label(*r == freq, format!("{r} Hz")).clicked() {
                            freq = *r;
                        }
                    }
                });
            });
            let changed = res != (mode.width, mode.height) || freq != mode.freq;
            self.pending_mode = changed.then(|| (m.key.clone(), res, freq));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_sized([170.0, 24.0], egui::Label::new("Rotação"));
                for (o, t) in [(0u32, "Paisagem"), (1, "Retrato"), (2, "Paisagem invertida"), (3, "Retrato invertido")] {
                    if ui.selectable_label(mode.orientation == o, t).clicked() && mode.orientation != o {
                        let r = win::apply_orientation(&m.gdi_name, o);
                        self.after_mode_change(m, &mode, r, t.to_lowercase());
                    }
                }
            });
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if ui.add_enabled(changed, egui::Button::new(format!("✔  Aplicar ao ecrã {}", m.win_number))).clicked() {
                    let r = win::apply_mode(&m.gdi_name, res.0, res.1, freq);
                    self.pending_mode = None;
                    self.after_mode_change(m, &mode, r, format!("{} x {} a {} Hz", res.0, res.1, freq));
                }
            });
        });
    }

    /// After a resolution/rotation change: refresh, and ask to keep it (reverts on its own).
    fn after_mode_change(&mut self, m: &MonitorDesc, before: &ModeInfo, result: Result<(), String>, what: String) {
        match result {
            Ok(()) => {
                self.status = format!("Ecrã {}: {what}.", m.win_number);
                self.status_bad = false;
                self.keep = Some(KeepMode {
                    key: m.key.clone(),
                    gdi: m.gdi_name.clone(),
                    number: m.win_number,
                    before: before.clone(),
                    what,
                    deadline: Instant::now() + KEEP_SECS,
                });
            }
            Err(e) => {
                self.status = format!("Ecrã {}: {e}", m.win_number);
                self.status_bad = true;
            }
        }
        self.modes.insert(m.key.clone(), win::mode_info(&m.gdi_name));
        let _ = self.tx.send(Cmd::Scan);
    }

    fn revert_mode(&mut self, k: KeepMode) {
        let now = win::mode_info(&k.gdi);
        let b = &k.before;
        let r = if now.orientation != b.orientation {
            win::apply_orientation(&k.gdi, b.orientation)
        } else {
            win::apply_mode(&k.gdi, b.width, b.height, b.freq)
        };
        match r {
            Ok(()) => {
                self.status = format!("Ecrã {}: configuração anterior reposta.", k.number);
                self.status_bad = false;
            }
            Err(e) => {
                self.status = format!("Ecrã {}: não foi possível repor ({e}).", k.number);
                self.status_bad = true;
            }
        }
        self.modes.insert(k.key.clone(), win::mode_info(&k.gdi));
        let _ = self.tx.send(Cmd::Scan);
    }

    fn modals(&mut self, ctx: &egui::Context) {
        if self.confirm_reset {
            if let Some(m) = self.current() {
                let mut close = false;
                let resp = egui::Modal::new(egui::Id::new("reset")).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.heading("Repor definições de fábrica?");
                    ui.add_space(6.0);
                    ui.label(format!(
                        "Todas as definições de imagem, jogo e entrada do ecrã {} ({}, S/N {}) voltam aos valores de fábrica. Os outros ecrãs não mudam.",
                        m.win_number,
                        self.display_name(&m),
                        m.serial_display()
                    ));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("Sim, repor").color(WARN)).clicked() {
                            // Only the selected monitor, never its followers.
                            let _ = self.tx.send(Cmd::Set(vec![m.key.clone()], vcp::RESTORE_FACTORY, 1));
                            self.values.remove(&m.key);
                            self.request_read(&m.key);
                            self.status = format!("Ecrã {} reposto de fábrica.", m.win_number);
                            self.status_bad = false;
                            close = true;
                        }
                        if ui.button("Cancelar").clicked() {
                            close = true;
                        }
                    });
                });
                if close || resp.should_close() {
                    self.confirm_reset = false;
                }
            } else {
                self.confirm_reset = false;
            }
        }

        if let Some(k) = &self.keep {
            let left = k.deadline.saturating_duration_since(Instant::now());
            let (mut keep, mut revert) = (false, left.is_zero());
            let (number, what) = (k.number, k.what.clone());
            ctx.request_repaint_after(Duration::from_millis(250));
            egui::Modal::new(egui::Id::new("keep")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("Manter esta configuração?");
                ui.add_space(6.0);
                ui.label(format!("O ecrã {number} está agora em {what}."));
                ui.label(RichText::new(format!("Volta ao que estava em {} s se não confirmar.", left.as_secs() + 1)).color(MUTED));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Manter").clicked() {
                        keep = true;
                    }
                    if ui.button("Voltar ao anterior").clicked() {
                        revert = true;
                    }
                });
            });
            if keep {
                self.keep = None;
            } else if revert {
                if let Some(k) = self.keep.take() {
                    self.revert_mode(k);
                }
            }
        }
    }
}

impl App {
    fn feature_row(&mut self, ui: &mut egui::Ui, m: &MonitorDesc, f: &vcp::Feature) {
        let read = self.values.get(&m.key).and_then(|v| v.get(&f.code)).copied();
        ui.horizontal(|ui| {
            ui.add_sized([170.0, 24.0], egui::Label::new(f.name));
            match (&f.kind, read) {
                (_, None) => {
                    ui.label(RichText::new("a ler…").color(MUTED));
                }
                (_, Some(None)) => {
                    ui.label(RichText::new("sem resposta").color(MUTED));
                }
                (Kind::Slider, Some(Some((cur, max)))) => {
                    let max = if max == 0 { 100 } else { max };
                    let mut v = cur.min(max);
                    let r = ui.add(egui::Slider::new(&mut v, 0..=max));
                    if r.changed() {
                        self.set_vcp(f.code, v);
                    }
                }
                (Kind::Choice(opts), Some(Some((cur, _)))) => {
                    let caps = self.caps.get(&m.key);
                    let allowed = caps.and_then(|c| c.values(f.code)).cloned();
                    let mut items: Vec<(u32, String)> = opts
                        .iter()
                        .filter(|(v, _)| allowed.as_ref().map(|a| a.contains(v)).unwrap_or(true))
                        .map(|(v, n)| (*v, n.to_string()))
                        .collect();
                    if let Some(a) = &allowed {
                        for v in a {
                            if !items.iter().any(|(x, _)| x == v) {
                                items.push((*v, format!("Valor 0x{v:02X}")));
                            }
                        }
                    }
                    let cur_masked = if f.code == 0x60 { cur & 0xFF } else { cur };
                    if !items.iter().any(|(x, _)| *x == cur_masked) {
                        items.insert(0, (cur_masked, format!("Valor 0x{cur_masked:02X}")));
                    }
                    let current = items.iter().find(|(x, _)| *x == cur_masked).map(|x| x.1.clone()).unwrap_or_default();
                    let mut chosen = None;
                    egui::ComboBox::from_id_salt((f.code, &m.key)).width(240.0).selected_text(current).show_ui(ui, |ui| {
                        for (v, n) in &items {
                            if ui.selectable_label(*v == cur_masked, n).clicked() {
                                chosen = Some(*v);
                            }
                        }
                    });
                    if let Some(v) = chosen {
                        if v != cur_masked {
                            self.set_vcp(f.code, v);
                        }
                    }
                }
            }
        });
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(s) = &mut self.shot else { return };
        s.frames += 1;
        ctx.request_repaint();
        if s.frames >= 120 && !s.requested {
            s.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                if let Some(dir) = s.path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Err(e) = buf.save(&s.path) {
                    eprintln!("screenshot save failed: {e}");
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let shot = args.iter().position(|a| a == "--screenshot").and_then(|i| args.get(i + 1)).map(PathBuf::from);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_inner_size([1100.0, 720.0])
        .with_min_inner_size([900.0, 560.0])
        .with_icon(std::sync::Arc::new(
            eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).expect("valid icon png"),
        ));
    if shot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(windows)]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(APP_NAME, options, Box::new(move |cc| Ok(Box::new(App::new(cc, shot)))))
}

