//! Scripted input for end-to-end testing (`XYLEM_SCRIPT`).
//!
//! Steps are separated by `;;`:
//! `wait N` · `type TEXT` · `paste TEXT` · `key NAME [ctrl] [shift] [alt]` · `click X Y` ·
//! `dblclick X Y` · `rclick X Y` · `hover X Y` · `shot PATH` · `drop FILE` · `save PATH` (writes the active
//! document's XML) · `pick PATH` (the answer the next file dialog gives; `pick -` cancels it) ·
//! `quit`.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

/// Answers for file dialogs, queued by `pick` steps.
static PICKS: Mutex<VecDeque<Option<PathBuf>>> = Mutex::new(VecDeque::new());

/// Shows a file dialog, unless a script has queued the answer.
pub fn file_dialog(show: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    if let Some(answer) = PICKS.lock().ok().and_then(|mut q| q.pop_front()) {
        return answer;
    }
    show()
}

pub enum Step {
    Wait(u32),
    Type(String),
    Paste(String),
    Key(Key, Modifiers),
    Click(Pos2, PointerButton, u8),
    CtrlClick(Pos2),
    /// Moves the pointer without clicking (shows tooltips).
    Hover(Pos2),
    Shot(PathBuf),
    Save(PathBuf),
    Drop(PathBuf),
    Quit,
}

pub struct Script {
    steps: VecDeque<Step>,
    wait: u32,
    pub pending_shot: Option<PathBuf>,
    pub pending_save: Option<PathBuf>,
    pub quit: bool,
    release: Option<(Pos2, PointerButton)>,
    hold_ctrl: bool,
}

impl Script {
    pub fn from_env() -> Option<Self> {
        let text = std::env::var("XYLEM_SCRIPT").ok()?;
        let mut steps = VecDeque::from([Step::Wait(20)]);
        for part in text.split(";;").map(str::trim).filter(|s| !s.is_empty()) {
            let (cmd, rest) = part.split_once(' ').unwrap_or((part, ""));
            let nums = || -> Vec<f32> {
                rest.split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect()
            };
            let step = match cmd {
                "wait" => Step::Wait(rest.trim().parse().unwrap_or(10)),
                "type" => Step::Type(rest.to_string()),
                // `\n` in the script stands for a line break in the pasted text.
                "paste" => Step::Paste(rest.replace("\\n", "\n")),
                "key" => {
                    let mut it = rest.split_whitespace();
                    let Some(k) = it.next().and_then(Key::from_name) else {
                        log::warn!("script: unknown key in `{part}`");
                        continue;
                    };
                    let mut m = Modifiers::NONE;
                    for w in it {
                        match w {
                            "ctrl" => m |= Modifiers::COMMAND,
                            "shift" => m |= Modifiers::SHIFT,
                            "alt" => m |= Modifiers::ALT,
                            _ => {}
                        }
                    }
                    Step::Key(k, m)
                }
                "hover" => {
                    let n = nums();
                    if n.len() < 2 {
                        continue;
                    }
                    Step::Hover(Pos2::new(n[0], n[1]))
                }
                "click" | "dblclick" | "rclick" | "ctrlclick" => {
                    let n = nums();
                    if n.len() < 2 {
                        continue;
                    }
                    let b = if cmd == "rclick" {
                        PointerButton::Secondary
                    } else {
                        PointerButton::Primary
                    };
                    let p = Pos2::new(n[0], n[1]);
                    match cmd {
                        "ctrlclick" => Step::CtrlClick(p),
                        "dblclick" => Step::Click(p, b, 2),
                        _ => Step::Click(p, b, 1),
                    }
                }
                "shot" => Step::Shot(PathBuf::from(rest.trim())),
                "save" => Step::Save(PathBuf::from(rest.trim())),
                "drop" => Step::Drop(PathBuf::from(rest.trim())),
                "quit" => Step::Quit,
                "pick" => {
                    // Queued now and consumed in order by whatever dialogs open.
                    let answer = (rest.trim() != "-").then(|| PathBuf::from(rest.trim()));
                    if let Ok(mut q) = PICKS.lock() {
                        q.push_back(answer);
                    }
                    continue;
                }
                _ => {
                    log::warn!("script: unknown step `{part}`");
                    continue;
                }
            };
            steps.push_back(step);
            steps.push_back(Step::Wait(4));
        }
        Some(Self {
            steps,
            wait: 0,
            pending_shot: None,
            pending_save: None,
            quit: false,
            release: None,
            hold_ctrl: false,
        })
    }

    /// Injects the next step's events into this frame's raw input.
    pub fn feed(&mut self, raw: &mut RawInput) {
        // Scripted runs drive the window as if it had OS focus.
        raw.focused = true;
        if let Some((p, b)) = self.release.take() {
            let m = if std::mem::take(&mut self.hold_ctrl) {
                Modifiers::COMMAND
            } else {
                Modifiers::NONE
            };
            raw.modifiers = m;
            raw.events.push(Event::PointerButton {
                pos: p,
                button: b,
                pressed: false,
                modifiers: m,
            });
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        let Some(step) = self.steps.pop_front() else {
            return;
        };
        match step {
            Step::Wait(n) => self.wait = n,
            Step::Type(t) => raw.events.push(Event::Text(t)),
            Step::Paste(t) => raw.events.push(Event::Paste(t)),
            Step::Key(k, m) => {
                raw.modifiers = m;
                let ev = match (k, m.command) {
                    (Key::C, true) => Event::Copy,
                    (Key::X, true) => Event::Cut,
                    _ => Event::Key {
                        key: k,
                        physical_key: Some(k),
                        pressed: true,
                        repeat: false,
                        modifiers: m,
                    },
                };
                raw.events.push(ev);
                raw.events.push(Event::Key {
                    key: k,
                    physical_key: Some(k),
                    pressed: false,
                    repeat: false,
                    modifiers: m,
                });
            }
            Step::Click(p, b, count) => {
                raw.events.push(Event::PointerMoved(p));
                raw.events.push(Event::PointerButton {
                    pos: p,
                    button: b,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                });
                self.release = Some((p, b));
                if count > 1 {
                    self.steps.push_front(Step::Click(p, b, count - 1));
                }
            }
            Step::CtrlClick(p) => {
                raw.modifiers = Modifiers::COMMAND;
                raw.events.push(Event::PointerMoved(p));
                raw.events.push(Event::PointerButton {
                    pos: p,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::COMMAND,
                });
                self.release = Some((p, PointerButton::Primary));
                self.hold_ctrl = true;
            }
            Step::Hover(p) => raw.events.push(Event::PointerMoved(p)),
            Step::Shot(p) => self.pending_shot = Some(p),
            Step::Save(p) => self.pending_save = Some(p),
            Step::Drop(p) => raw.dropped_files.push(egui::DroppedFile {
                path: Some(p),
                ..Default::default()
            }),
            Step::Quit => self.quit = true,
        }
    }
}
