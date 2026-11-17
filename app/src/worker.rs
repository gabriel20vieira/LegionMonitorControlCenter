//! Background thread that owns the DDC/CI handles. DDC/CI is slow (tens of
//! milliseconds per call), so the window never waits on it.
//! Every command names its monitors by the unique key, never by model name.

use crate::win::{self, MonitorDesc, HANDLE};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};

pub enum Cmd {
    Scan,
    Read(String),
    Caps(String),
    Set(Vec<String>, u8, u32),
}

pub enum Msg {
    Monitors(Vec<MonitorDesc>),
    Value(String, u8, Option<(u32, u32)>),
    ReadDone(String),
    Caps(String, Option<String>),
    SetDone { code: u8, value: u32, ok: Vec<String>, failed: Vec<String> },
}

pub fn spawn(ctx: eframe::egui::Context) -> (Sender<Cmd>, Receiver<Msg>) {
    let (tx_cmd, rx_cmd) = channel::<Cmd>();
    let (tx_msg, rx_msg) = channel::<Msg>();
    std::thread::Builder::new()
        .name("ddc".into())
        .spawn(move || {
            let mut handles: HashMap<String, HANDLE> = HashMap::new();
            let send = |m: Msg| {
                let _ = tx_msg.send(m);
                ctx.request_repaint();
            };
            while let Ok(first) = rx_cmd.recv() {
                // Drain and coalesce: only the newest value per (monitors, code) is sent.
                let mut queue = vec![first];
                while let Ok(c) = rx_cmd.try_recv() {
                    if let Cmd::Set(keys, code, _) = &c {
                        queue.retain(|q| !matches!(q, Cmd::Set(k2, c2, _) if k2 == keys && c2 == code));
                    }
                    queue.push(c);
                }
                for cmd in queue {
                    match cmd {
                        Cmd::Scan => {
                            for (_, h) in handles.drain() {
                                win::destroy_handle(h);
                            }
                            let found = win::enumerate();
                            let mut list = Vec::new();
                            for (desc, h) in found {
                                if let Some(h) = h {
                                    handles.insert(desc.key.clone(), h);
                                }
                                list.push(desc);
                            }
                            send(Msg::Monitors(list));
                        }
                        Cmd::Read(key) => {
                            if let Some(&h) = handles.get(&key) {
                                for code in crate::vcp::read_order() {
                                    let v = win::get_vcp(h, code);
                                    send(Msg::Value(key.clone(), code, v));
                                }
                            }
                            send(Msg::ReadDone(key));
                        }
                        Cmd::Caps(key) => {
                            let caps = handles.get(&key).and_then(|&h| win::capabilities(h));
                            send(Msg::Caps(key, caps));
                        }
                        Cmd::Set(keys, code, value) => {
                            let (mut ok, mut failed) = (Vec::new(), Vec::new());
                            for k in keys {
                                match handles.get(&k) {
                                    Some(&h) if win::set_vcp(h, code, value) => ok.push(k),
                                    _ => failed.push(k),
                                }
                            }
                            send(Msg::SetDone { code, value, ok, failed });
                        }
                    }
                }
            }
            for (_, h) in handles.drain() {
                win::destroy_handle(h);
            }
        })
        .expect("ddc thread");
    (tx_cmd, rx_msg)
}
