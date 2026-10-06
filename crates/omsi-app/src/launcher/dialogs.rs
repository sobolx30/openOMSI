//! The launcher's dialogs over the page: the server ended the game, the game closed on an error.

use super::theme::*;
use super::ui::ButtonKind;
use super::Launcher;
use crate::version;
use glam::Vec2;
use omsi_ui::paint::Align;
use omsi_ui::{Rect, Weight};

/// The run went down while a Vulkan driver compiled the shaders: the LAST it said was a stage
/// of that, and it drew with Vulkan (as the phone's shell decides it, `android.rs`). Any
/// compile stage anywhere in the log said so of every silent end - a phone run out of memory
/// 75 % into loading a map on OpenGL was told its Vulkan driver had failed (#848).
pub(crate) fn died_compiling_on_vulkan(log: &str) -> bool {
    let last = log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
    let compiling = last.contains("renderer: compiling") || last.contains("cloud noise made") || last.contains("opening graphics device") || last.contains("compiling renderer pipelines");
    let vulkan = log.lines().any(|l| (l.contains("renderer: ") || l.contains("opening graphics device")) && l.contains("(Vulkan")) || log.lines().any(|l| l.contains("graphics: ") && l.to_ascii_uppercase().contains("VULKAN"));
    compiling && vulkan
}

#[cfg(test)]
mod hint_tests {
    #[test]
    fn only_a_run_that_died_compiling_on_vulkan_is_told_so() {
        let compiled = "[t INFO r] opening graphics device: Mali (Vulkan, vendor 0x13b5)\n[t INFO r] renderer: compiling the scene shaders\n";
        assert!(super::died_compiling_on_vulkan(compiled));
        // compiled long ago, then ran out of memory loading
        assert!(!super::died_compiling_on_vulkan(&format!("{compiled}[t INFO g] status: 63 fps, view driver\n[t INFO m] loading tiles 75 %\n")));
        // on OpenGL it is never the Vulkan driver
        assert!(!super::died_compiling_on_vulkan("[t INFO r] opening graphics device: Mali (Gl, vendor 0x13b5)\n[t INFO r] renderer: compiling the scene shaders\n"));
    }
}

impl Launcher {
    /// A game started from here was sent away by its server (kicked, banned) or turned away at
    /// the door: the game is over, and this says so with the server's own message.
    pub(super) fn draw_disconnect_dialog(&mut self) {
        let Some(why) = self.state.disconnected.clone() else { return };
        let size = self.ui.size;
        let full = Rect::new(0.0, 0.0, size.x, size.y);
        self.ui.solid(full);
        self.ui.p().rect(full, omsi_ui::Color::rgba(0, 0, 0, 0.62));
        let w = (size.x - 48.0).min(560.0);
        let lead = omsi_ui::tr("The server ended your game. Its message:");
        let th = self.ui.paragraph_height(&why, w - 48.0, 14.0, Weight::Regular).min(size.y * 0.4);
        let h = (176.0 + th).min(size.y - 24.0);
        let r = Rect::new((size.x - w) * 0.5, (size.y - h) * 0.5, w, h);
        self.ui.panel(r);
        let inner = Rect::new(r.x + 24.0, r.y + 20.0, r.w - 48.0, r.h - 40.0);
        self.ui.icon("error", Vec2::new(inner.x + 14.0, inner.y + 14.0), 26.0, DANGER);
        self.ui.text_in("Disconnected from the server", Rect::new(inner.x + 38.0, inner.y, inner.w - 38.0, 28.0), 18.0, Weight::Bold, TEXT, Align::Left);
        self.ui.paragraph(&lead, Vec2::new(inner.x, inner.y + 40.0), inner.w, 13.0, Weight::Regular, TEXT_DIM);
        self.ui.push_clip(Rect::new(inner.x, inner.y + 66.0, inner.w, th + 4.0), 0.0);
        self.ui.paragraph(&why, Vec2::new(inner.x, inner.y + 66.0), inner.w, 14.0, Weight::Bold, TEXT);
        self.ui.pop_clip();
        let by = inner.bottom() - 38.0;
        if self.ui.button("disconnect-close", Rect::new(inner.right() - 110.0, by, 110.0, 38.0), "Close", None, ButtonKind::Primary) {
            self.state.disconnected = None;
        }
    }

    /// A game started from here ended on an error: what it said, and the ways to report it
    /// (the end of its log copied, or a GitHub issue opened with it).
    pub(super) fn draw_crash_dialog(&mut self) {
        let Some((what, tail)) = self.state.crash.clone() else { return };
        let size = self.ui.size;
        let full = Rect::new(0.0, 0.0, size.x, size.y);
        self.ui.solid(full);
        self.ui.p().rect(full, omsi_ui::Color::rgba(0, 0, 0, 0.62));
        let w = (size.x - 48.0).min(640.0);
        let lost = what.contains("graphics device was lost");
        let silent = what.contains("closed without a word");
        let compiling = silent && died_compiling_on_vulkan(&tail);
        let hint = if lost {
            if cfg!(windows) {
                "The graphics driver stopped the game. Updating the graphics driver usually helps; you can also let the game draw with DirectX 12 instead of Vulkan (the button below, or Settings → Graphics API)."
            } else {
                "The graphics driver stopped the game. Updating the graphics driver usually helps; Settings → Graphics API can switch to OpenGL."
            }
        } else if compiling {
            "The Vulkan graphics driver stopped while compiling shaders. Starting the game again will switch to OpenGL (or change it in Settings → Graphics API)."
        } else if silent {
            "The system closed the game while it was running, typically because the device ran out of memory (RAM). Lowering texture resolution or reducing AI traffic in Settings helps prevent memory exhaustion."
        } else {
            "Copy the report (the end of the game's log), or open a GitHub issue with it: it tells what went wrong on this computer."
        };
        let text = format!("{what}\n\n{hint}");
        let th = self.ui.paragraph_height(&text, w - 48.0, 13.0, Weight::Regular).min(size.y * 0.5);
        let h = (140.0 + th).min(size.y - 24.0);
        let r = Rect::new((size.x - w) * 0.5, (size.y - h) * 0.5, w, h);
        self.ui.panel(r);
        let inner = Rect::new(r.x + 24.0, r.y + 20.0, r.w - 48.0, r.h - 40.0);
        self.ui.icon(if silent { "info" } else { "error" }, Vec2::new(inner.x + 14.0, inner.y + 14.0), 26.0, if silent { WARN } else { DANGER });
        let title_text = if silent { "The game was closed by the system" } else { "The game closed on an error" };
        self.ui.text_in(title_text, Rect::new(inner.x + 38.0, inner.y, inner.w - 38.0, 28.0), 18.0, Weight::Bold, TEXT, Align::Left);
        self.ui.push_clip(Rect::new(inner.x, inner.y + 40.0, inner.w, th + 4.0), 0.0);
        self.ui.paragraph(&text, Vec2::new(inner.x, inner.y + 40.0), inner.w, 13.0, Weight::Regular, TEXT_DIM);
        self.ui.pop_clip();
        let by = inner.bottom() - 38.0;
        if self.ui.button("crash-close", Rect::new(inner.right() - 110.0, by, 110.0, 38.0), "Close", None, ButtonKind::Normal) {
            self.state.crash = None;
        }
        if self.ui.button("crash-copy", Rect::new(inner.right() - 270.0, by, 150.0, 38.0), "Copy report", Some("content_copy"), ButtonKind::Primary) {
            self.ui.clipboard_out = Some(format!("openOMSI {} ({})\n{what}\n\n{tail}", version::current_version(), std::env::consts::OS));
            self.state.set_status("The report is copied: paste it into a GitHub issue or a message.", false);
        }
        let api = self.state.settings.get("graphics_api").and_then(|v| v.as_str()).unwrap_or("auto").to_string();
        if lost && cfg!(windows) && api != "dx12" && self.ui.button("crash-dx12", Rect::new(inner.x + 200.0, by, 170.0, 38.0), "Use DirectX 12", Some("monitor"), ButtonKind::Normal) {
            self.state.settings["graphics_api"] = serde_json::json!("dx12");
            self.state.settings_dirty = 0.3;
            self.state.crash = None;
            self.state.set_status("The game draws with DirectX 12 from the next start (Settings → Graphics API to change it back).", false);
        }
        if silent {
            if self.ui.button("crash-settings", Rect::new(inner.x, by, 150.0, 38.0), "Settings", Some("tune"), ButtonKind::Ghost) {
                self.state.crash = None;
                self.go(super::Page::Settings);
            }
        } else if self.ui.button("crash-issue", Rect::new(inner.x, by, 190.0, 38.0), "Report on GitHub", Some("open_in_new"), ButtonKind::Ghost) {
            let title = format!("Crash: {}", what.chars().take(80).collect::<String>());
            let enc = |t: &str| t.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
            // the end of the log goes with it, as much as a link holds (a report of the
            // last line alone said where the game stopped, never what led there); the whole
            // report is on the clipboard as well
            // (the computer and the map always, see `crash_of`)
            let (machine, end) = tail.split_once(&format!("\n{}\n", super::state::CRASH_TAIL_GAP)).unwrap_or(("", &tail));
            let machine = if machine.is_empty() { String::new() } else { format!("The computer:\n```\n{machine}\n```\n\n") };
            let body_with = |end: &str| format!("openOMSI {} on {}\n\n```\n{what}\n```\n\n{machine}The end of the log:\n```\n{end}\n```\n", version::current_version(), std::env::consts::OS);
            let lines: Vec<&str> = end.lines().collect();
            let mut shown = 0;
            let body = loop {
                let body = body_with(&lines[lines.len() - shown..].join("\n"));
                if shown >= lines.len() || enc(&body).len() > 6500 {
                    break if shown == 0 { body } else { body_with(&lines[lines.len() - shown.saturating_sub(1)..].join("\n")) };
                }
                shown += 1;
            };
            self.ui.clipboard_out = Some(format!("openOMSI {} ({})\n{what}\n\n{tail}", version::current_version(), std::env::consts::OS));
            version::open_url(&format!("{}/issues/new?title={}&body={}", version::REPO_URL, enc(&title), enc(&body)));
        }
    }
}
