//! Scripted input for end-to-end testing (`XYLEM_SCRIPT`).
//!
//! Steps are separated by `;;`. Prefer the steps that find their target by
//! name or text, so a script keeps working when the layout changes:
//!
//! * `cmd NAME [ARG]`: run an app command (`cmd SaveAs`, `cmd View normal`,
//!   `cmd InsertElement para`; see `Cmd::from_script`).
//! * `caret TEXT`, `caretend TEXT`, `select TEXT`: put the caret before or
//!   after, or select, the first occurrence of TEXT in the document.
//! * `clickat DX DY TEXT` (also `dblclickat`, `rclickat`, `ctrlclickat`):
//!   click DX, DY points from where the caret sits before TEXT.
//! * `clickui LABEL` (also `dblclickui`, `rclickui`, `ctrlclickui`): click a
//!   widget by its label or tooltip; the last one drawn wins.
//! * `pick PATH`: the answer the next file dialog gives (`pick -` cancels).
//!   Scripted runs never show a real file dialog.
//! * `type TEXT`, `paste TEXT`, `key NAME [ctrl] [shift] [alt]`.
//! * `wait N` (frames), `shot PATH`, `save PATH` (the active document's
//!   XML), `drop FILE`, `crash` (exit at once, as a crash would), `panic`
//!   (a real panic, for the crash report), `quit`.
//! * Coordinates, for when nothing else will do: `click X Y`, `dblclick`,
//!   `rclick`, `ctrlclick`, `hover`.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

/// Answers for file dialogs, queued by `pick` steps.
static PICKS: Mutex<VecDeque<Option<PathBuf>>> = Mutex::new(VecDeque::new());

/// Shows a file dialog, unless a script has queued the answer.
/// In a scripted run a native dialog is never shown: with nothing queued
/// the dialog counts as cancelled, so a test can't block on a window that
/// would also pop up in front of whoever is using the machine.
pub fn file_dialog(show: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    if let Some(answer) = PICKS.lock().ok().and_then(|mut q| q.pop_front()) {
        return answer;
    }
    if ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        log::warn!("script: a file dialog opened with no `pick` queued; treated as cancelled");
        return None;
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
    Crash,
    Panic,
    /// Runs an app command by name (`cmd SaveAs`, `cmd InsertElement para`).
    Command(String, String),
    /// Puts the caret at, or selects, the first occurrence of some text.
    Caret(String, CaretAt),
    /// Clicks relative to where the caret was last drawn (see `clickat`).
    ClickAnchor(egui::Vec2, ClickKind),
    /// Clicks the widget registered under this label (see [`track`]).
    ClickUi(String, ClickKind),
}

/// Where a `caret`, `caretend` or `select` step puts the selection.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CaretAt {
    Start,
    End,
    Select,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClickKind {
    Click,
    Double,
    Right,
    Ctrl,
}

impl ClickKind {
    fn from_prefix(cmd: &str) -> Self {
        if cmd.starts_with("dbl") {
            Self::Double
        } else if cmd.starts_with("r") {
            Self::Right
        } else if cmd.starts_with("ctrl") {
            Self::Ctrl
        } else {
            Self::Click
        }
    }
}

static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Widgets drawn in the last frame, by label, for `clickui` steps.
static UI_RECTS: Mutex<Vec<(String, egui::Rect)>> = Mutex::new(Vec::new());

/// Registers a widget so a script can click it by label (`clickui Save`).
/// Free when no script is running. The shared UI components call this, so
/// most buttons are reachable without fixed coordinates.
pub fn track(label: &str, response: &egui::Response) {
    if !ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let Ok(mut v) = UI_RECTS.lock() {
        v.push((label.to_string(), response.rect));
    }
}

/// Starts a new frame's widget registry; the previous frame's stays
/// readable until then.
pub fn begin_frame() -> Vec<(String, egui::Rect)> {
    UI_RECTS
        .lock()
        .map(|mut v| std::mem::take(&mut *v))
        .unwrap_or_default()
}

pub struct Script {
    steps: VecDeque<Step>,
    wait: u32,
    pub pending_shot: Option<PathBuf>,
    pub pending_save: Option<PathBuf>,
    /// Commands for the app to run this frame: (name, argument).
    pub pending_cmds: Vec<(String, String)>,
    pub pending_caret: Option<(String, CaretAt)>,
    /// The caret's rectangle on screen in the last frame, set by the app.
    pub anchor: Option<egui::Rect>,
    /// Widgets registered in the last frame, set by the app.
    pub ui_rects: Vec<(String, egui::Rect)>,
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
                "cmd" => {
                    let (name, arg) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
                    Step::Command(name.to_string(), arg.to_string())
                }
                "caret" => Step::Caret(rest.to_string(), CaretAt::Start),
                "caretend" => Step::Caret(rest.to_string(), CaretAt::End),
                "select" => Step::Caret(rest.to_string(), CaretAt::Select),
                // `clickat DX DY TEXT`: put the caret before TEXT, then click
                // DX, DY points from the top-left of the caret.
                "clickat" | "dblclickat" | "rclickat" | "ctrlclickat" => {
                    let mut it = rest.splitn(3, ' ');
                    let (Some(dx), Some(dy), Some(text)) = (
                        it.next().and_then(|v| v.parse::<f32>().ok()),
                        it.next().and_then(|v| v.parse::<f32>().ok()),
                        it.next(),
                    ) else {
                        log::warn!("script: expected `{cmd} DX DY TEXT` in `{part}`");
                        continue;
                    };
                    steps.push_back(Step::Caret(text.to_string(), CaretAt::Start));
                    steps.push_back(Step::Wait(3));
                    Step::ClickAnchor(egui::vec2(dx, dy), ClickKind::from_prefix(cmd))
                }
                "clickui" | "dblclickui" | "rclickui" | "ctrlclickui" => {
                    Step::ClickUi(rest.trim().to_string(), ClickKind::from_prefix(cmd))
                }
                "shot" => Step::Shot(PathBuf::from(rest.trim())),
                "save" => Step::Save(PathBuf::from(rest.trim())),
                "drop" => Step::Drop(PathBuf::from(rest.trim())),
                "quit" => Step::Quit,
                // Ends the process on the spot, as a crash would: no save
                // prompt, no settings written (crash-recovery tests).
                "crash" => Step::Crash,
                // A real panic, to test the crash report.
                "panic" => Step::Panic,
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
        ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
        Some(Self {
            steps,
            wait: 0,
            pending_shot: None,
            pending_save: None,
            pending_cmds: Vec::new(),
            pending_caret: None,
            anchor: None,
            ui_rects: Vec::new(),
            quit: false,
            release: None,
            hold_ctrl: false,
        })
    }

    /// Injects the next step's events into this frame's raw input.
    pub fn feed(&mut self, raw: &mut RawInput) {
        // Scripted runs drive the window as if it had OS focus, and only by
        // the script: real keyboard and mouse input is dropped, so whatever
        // the person at the machine is doing (in another window) can never
        // leak into a test, and a test never depends on where the mouse is.
        raw.focused = true;
        raw.events.retain(|e| {
            !matches!(
                e,
                Event::Text(_)
                    | Event::Key { .. }
                    | Event::Paste(_)
                    | Event::Copy
                    | Event::Cut
                    | Event::Ime(_)
                    | Event::PointerMoved(_)
                    | Event::MouseMoved(_)
                    | Event::PointerButton { .. }
                    | Event::PointerGone
                    | Event::MouseWheel { .. }
                    | Event::Zoom(_)
                    | Event::Touch { .. }
            )
        });
        raw.modifiers = Modifiers::NONE;
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
            Step::Crash => std::process::exit(3),
            Step::Panic => panic!("panic requested by a test script"),
            Step::Command(name, arg) => self.pending_cmds.push((name, arg)),
            Step::Caret(text, at) => self.pending_caret = Some((text, at)),
            Step::ClickAnchor(offset, kind) => match self.anchor {
                Some(r) => self.push_click(r.min + offset, kind),
                None => log::warn!("script: clickat with no caret on screen"),
            },
            Step::ClickUi(label, kind) => {
                // Prefer an exact label, then one that starts with it ("Save"
                // also finds "Save as…" only when nothing is called "Save").
                // Latest drawn first: dialogs and popups are drawn after the
                // panels, so they win when labels repeat ("Insert").
                let hit = self
                    .ui_rects
                    .iter()
                    .rev()
                    .find(|(l, _)| *l == label)
                    .or_else(|| {
                        self.ui_rects
                            .iter()
                            .rev()
                            .find(|(l, _)| l.starts_with(&label))
                    })
                    .map(|(_, r)| r.center());
                match hit {
                    Some(p) => self.push_click(p, kind),
                    None => log::warn!("script: no widget labelled `{label}` on screen"),
                }
            }
        }
    }

    /// Queues a click of the given kind at `p`, to be sent on the next frames.
    fn push_click(&mut self, p: Pos2, kind: ClickKind) {
        let step = match kind {
            ClickKind::Click => Step::Click(p, PointerButton::Primary, 1),
            ClickKind::Double => Step::Click(p, PointerButton::Primary, 2),
            ClickKind::Right => Step::Click(p, PointerButton::Secondary, 1),
            ClickKind::Ctrl => Step::CtrlClick(p),
        };
        self.steps.push_front(step);
    }
}
