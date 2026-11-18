#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod edid;
mod overlay;
mod store;
mod vcp;
mod win;
mod worker;

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Feature(Tab),
    Display,
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
    page: Page,
    status: String,
    status_bad: bool,
    nickname_edit: Option<String>,
    confirm_reset: bool,
    shot: Option<Shot>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, shot: Option<PathBuf>) -> Self {
        style(&cc.egui_ctx);
        let (tx, rx) = worker::spawn(cc.egui_ctx.clone());
        let _ = tx.send(Cmd::Scan);
        let store = store::Store::load();
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
            page: Page::Home,
            status: "A procurar monitores…".into(),
            status_bad: false,
            nickname_edit: None,
            confirm_reset: false,
            shot: shot.map(|path| Shot { path, frames: 0, requested: false }),
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
        self.selected = Some(key.clone());
        self.store.last_selected = Some(key);
        self.store.save();
        self.nickname_edit = None;
        self.confirm_reset = false;
        self.pending_mode = None;
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
                    self.status = match n {
                        0 => "Nenhum monitor encontrado.".into(),
                        1 => "1 monitor encontrado.".into(),
                        _ => format!("{n} monitores encontrados, cada um com o seu número do Windows e ID único."),
                    };
                    self.status_bad = n == 0;
                    let keep = self.selected.clone().filter(|k| keys.contains(k));
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
        self.screenshot_step(ctx);

        egui::TopBottomPanel::top("top")
            .exact_height(56.0)
            .frame(egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::symmetric(16, 8)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(6.0, 28.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, ACCENT);
                    ui.label(RichText::new("Monitor Control Center").size(20.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("⟳ Procurar monitores").clicked() {
                            self.status = "A procurar monitores…".into();
                            let _ = self.tx.send(Cmd::Scan);
                        }
                        if ui.button("Identificar").on_hover_text("Mostra o número do Windows em cada monitor").clicked() {
                            let items = self
                                .monitors
                                .iter()
                                .filter(|m| m.sub_index == 0)
                                .map(|m| {
                                    let cap = format!("{}\n{}", self.label(m).split(" · ").nth(1).unwrap_or(&m.name), m.instance_short);
                                    (m.rect, m.win_number.to_string(), cap, Some(&m.key) == self.selected.as_ref())
                                })
                                .collect();
                            overlay::identify(items, 2500);
                        }
                    });
                });
            });

        egui::TopBottomPanel::bottom("status")
            .exact_height(30.0)
            .frame(egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::symmetric(16, 6)))
            .show(ctx, |ui| {
                ui.label(RichText::new(&self.status).color(if self.status_bad { WARN } else { MUTED }));
            });

        egui::SidePanel::left("monitors")
            .exact_width(290.0)
            .resizable(false)
            .frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::same(12)))
            .show(ctx, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(Color32::from_rgb(0x23, 0x2f, 0x4a)).inner_margin(egui::Margin::same(20)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.main_page(ui));
            });
    }
}

impl App {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("MONITORES").color(MUTED).size(13.0).strong());
        ui.add_space(2.0);
        if !self.scanned {
            ui.spinner();
        }
        let mut clicked: Option<String> = None;
        let mut toggled: Option<String> = None;
        let list = self.monitors.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for m in &list {
                let selected = self.selected.as_deref() == Some(m.key.as_str());
                let fill = if selected { Color32::from_rgb(0x37, 0x59, 0x94) } else { CARD };
                egui::Frame::default()
                    .fill(fill)
                    .stroke(Stroke::new(1.0_f32, if selected { ACCENT } else { PANEL }))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let header = ui.horizontal(|ui| {
                            badge(ui, m.win_number, 34.0, if selected { ACCENT } else { PANEL });
                            ui.vertical(|ui| {
                                let lbl = self.label(m);
                                let name = lbl.split_once(" · ").map(|x| x.1.to_string()).unwrap_or(lbl);
                                ui.label(RichText::new(name).strong());
                                ui.label(RichText::new(format!("S/N {}  ·  {}", m.serial_display(), m.instance_short)).size(12.0).color(MUTED));
                                ui.horizontal(|ui| {
                                    if m.primary {
                                        ui.label(RichText::new("principal").size(11.0).color(ACCENT));
                                    }
                                    if self.twins(m) > 0 {
                                        ui.label(RichText::new("modelo igual a outro").size(11.0).color(Color32::from_rgb(0xff, 0xc1, 0x4d)));
                                    }
                                    if !m.ddc {
                                        ui.label(RichText::new("sem DDC/CI").size(11.0).color(WARN));
                                    }
                                });
                            });
                        });
                        // Only the header selects the monitor, so the "follow" checkbox below keeps its own clicks.
                        let pick = ui
                            .interact(header.response.rect, ui.id().with(("pick", &m.key)), Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if pick.clicked() {
                            clicked = Some(m.key.clone());
                        }
                        if !selected && m.ddc && self.selected.is_some() {
                            let mut on = self.store.sync.contains(&m.key);
                            if ui.checkbox(&mut on, RichText::new("Seguir alterações do selecionado").size(12.0)).changed() {
                                toggled = Some(m.key.clone());
                            }
                        }
                    });
                ui.add_space(4.0);
            }
        });
        if let Some(k) = toggled {
            if !self.store.sync.remove(&k) {
                self.store.sync.insert(k);
            }
            self.store.save();
        }
        if let Some(k) = clicked {
            self.select(k);
        }
    }

    fn main_page(&mut self, ui: &mut egui::Ui) {
        let Some(key) = self.selected.clone() else {
            ui.label("Escolha um monitor à esquerda.");
            return;
        };
        let Some(m) = self.monitor(&key).cloned() else {
            ui.label("O monitor selecionado já não está ligado.");
            return;
        };

        // Header: number badge, name (nickname editable), unique identity line.
        ui.horizontal(|ui| {
            badge(ui, m.win_number, 52.0, ACCENT);
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
                            let lbl = self.label(&m);
                            let name = lbl.split_once(" · ").map(|x| x.1.to_string()).unwrap_or(lbl);
                            ui.heading(name);
                            if ui.small_button("✏ Nome").on_hover_text("Dê um nome próprio a este monitor").clicked() {
                                self.nickname_edit = Some(self.store.nicknames.get(&key).cloned().unwrap_or_default());
                            }
                        });
                    }
                }
                ui.label(
                    RichText::new(format!("Ecrã {} do Windows ({})  ·  ID {}", m.win_number, m.gdi_name, if m.instance_short.is_empty() { "—" } else { &m.instance_short }))
                        .color(MUTED),
                );
            });
        });
        if self.twins(&m) > 0 {
            ui.add_space(4.0);
            egui::Frame::default().fill(Color32::from_rgb(0x3a, 0x34, 0x22)).corner_radius(6.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.label(RichText::new(format!(
                    "Há {} outro(s) monitor(es) do mesmo modelo. As alterações aqui vão só para o ecrã {} (ID {}), a não ser que marque \"Seguir\" noutro.",
                    self.twins(&m),
                    m.win_number,
                    m.instance_short
                )).color(Color32::from_rgb(0xff, 0xd9, 0x8a)));
            });
        }
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            for (p, t) in [
                (Page::Home, "Início"),
                (Page::Feature(Tab::Image), "Imagem"),
                (Page::Feature(Tab::Game), "Jogo"),
                (Page::Feature(Tab::Input), "Entrada e energia"),
                (Page::Display, "Resolução e rotação"),
            ] {
                if ui.selectable_label(self.page == p, RichText::new(t).size(16.0)).clicked() {
                    self.page = p;
                }
            }
        });
        ui.separator();
        ui.add_space(6.0);

        match self.page {
            Page::Home => self.home(ui, &m),
            Page::Feature(tab) => self.features(ui, &m, tab),
            Page::Display => self.display(ui, &m),
        }
    }

    fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        egui::Frame::default()
            .fill(CARD)
            .corner_radius(8.0)
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(title).color(MUTED).strong());
                ui.add_space(4.0);
                add(ui)
            })
            .inner
    }

    fn home(&mut self, ui: &mut egui::Ui, m: &MonitorDesc) {
        let mode = self.modes.get(&m.key).cloned().unwrap_or_default();
        Self::card(ui, "INFORMAÇÃO DO MONITOR", |ui| {
            egui::Grid::new("info").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(RichText::new(k).color(MUTED));
                    ui.label(v);
                    ui.end_row();
                };
                row(ui, "Modelo", m.name.clone());
                row(ui, "Fabricante", if m.manufacturer.is_empty() { "—".into() } else { m.manufacturer.clone() });
                row(ui, "Número de série", m.serial_display());
                row(ui, "Número do Windows", format!("{}  ({})", m.win_number, m.gdi_name));
                row(ui, "ID único do dispositivo", if m.instance_path.is_empty() { "—".into() } else { m.instance_path.clone() });
                row(ui, "Resolução atual", if mode.width > 0 { format!("{} x {}", mode.width, mode.height) } else { "—".into() });
                row(ui, "Frequência", if mode.freq > 1 { format!("{} Hz", mode.freq) } else { "—".into() });
                row(ui, "Posição no ambiente de trabalho", format!("{}, {}  ({} x {})", m.rect.left, m.rect.top, m.width(), m.height()));
                row(ui, "Ano de fabrico", if m.year > 1990 { m.year.to_string() } else { "—".into() });
                row(ui, "Controlo DDC/CI", if m.ddc { "disponível".into() } else { "indisponível (ative DDC/CI no menu do monitor)".into() });
            });
        });
        ui.add_space(10.0);
        if m.ddc {
            Self::card(ui, "ATALHOS", |ui| {
                self.feature_row(ui, m, vcp::FEATURES.iter().find(|f| f.code == vcp::BRIGHTNESS).unwrap());
                self.feature_row(ui, m, vcp::FEATURES.iter().find(|f| f.code == vcp::CONTRAST).unwrap());
            });
        }
    }

    fn features(&mut self, ui: &mut egui::Ui, m: &MonitorDesc, tab: Tab) {
        if !m.ddc {
            ui.label(RichText::new("Este monitor não respondeu por DDC/CI. Ative a opção DDC/CI no menu do próprio monitor e carregue em \"Procurar monitores\".").color(WARN));
            return;
        }
        if self.reading.contains(&m.key) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("A ler definições do monitor…").color(MUTED));
            });
        }
        let caps = self.caps.get(&m.key).cloned().unwrap_or_default();
        let title = match tab {
            Tab::Image => "IMAGEM",
            Tab::Game => "JOGO",
            Tab::Input => "ENTRADA E ENERGIA",
        };
        Self::card(ui, title, |ui| {
            let mut shown = 0;
            for f in vcp::FEATURES.iter().filter(|f| f.tab == tab) {
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
            Self::card(ui, "REPOR DEFINIÇÕES DE FÁBRICA", |ui| {
                if !self.confirm_reset {
                    if ui.button("Repor de fábrica…").clicked() {
                        self.confirm_reset = true;
                    }
                } else {
                    ui.label(RichText::new(format!("Repor as definições de fábrica do ecrã {}?", m.win_number)).color(WARN));
                    ui.horizontal(|ui| {
                        if ui.button("Sim, repor").clicked() {
                            self.set_vcp(vcp::RESTORE_FACTORY, 1);
                            self.confirm_reset = false;
                            self.values.remove(&m.key);
                            self.request_read(&m.key);
                        }
                        if ui.button("Cancelar").clicked() {
                            self.confirm_reset = false;
                        }
                    });
                }
            });
        }
    }

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

    fn display(&mut self, ui: &mut egui::Ui, m: &MonitorDesc) {
        let mode = self.modes.get(&m.key).cloned().unwrap_or_default();
        Self::card(ui, "RESOLUÇÃO E FREQUÊNCIA", |ui| {
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
            ui.add_space(4.0);
            if ui.add_enabled(changed, egui::Button::new(format!("Aplicar ao ecrã {}", m.win_number))).clicked() {
                match win::apply_mode(&m.gdi_name, res.0, res.1, freq) {
                    Ok(()) => {
                        self.status = format!("Ecrã {}: {} x {} a {} Hz.", m.win_number, res.0, res.1, freq);
                        self.status_bad = false;
                    }
                    Err(e) => {
                        self.status = format!("Ecrã {}: {e}", m.win_number);
                        self.status_bad = true;
                    }
                }
                self.pending_mode = None;
                self.modes.insert(m.key.clone(), win::mode_info(&m.gdi_name));
                let _ = self.tx.send(Cmd::Scan);
            }
        });
        ui.add_space(10.0);
        Self::card(ui, "ROTAÇÃO (PIVÔ)", |ui| {
            ui.horizontal(|ui| {
                for (o, t) in [(0u32, "Paisagem"), (1, "Retrato"), (2, "Paisagem invertida"), (3, "Retrato invertido")] {
                    if ui.selectable_label(mode.orientation == o, t).clicked() && mode.orientation != o {
                        match win::apply_orientation(&m.gdi_name, o) {
                            Ok(()) => {
                                self.status = format!("Ecrã {} rodado: {t}.", m.win_number);
                                self.status_bad = false;
                            }
                            Err(e) => {
                                self.status = format!("Ecrã {}: {e}", m.win_number);
                                self.status_bad = true;
                            }
                        }
                        self.modes.insert(m.key.clone(), win::mode_info(&m.gdi_name));
                        let _ = self.tx.send(Cmd::Scan);
                    }
                }
            });
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
        .with_title("Monitor Control Center")
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
    eframe::run_native("Monitor Control Center", options, Box::new(move |cc| Ok(Box::new(App::new(cc, shot)))))
}
