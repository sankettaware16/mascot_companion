//! Desktop-notification listener. GNOME draws its notification banners inside
//! the shell (they're not X windows), so the X11 scout alone can miss them.
//! This watcher spawns `dbus-monitor` filtered to `org.freedesktop.Notifications
//! .Notify` and reports an event (with the sending app's name) whenever any
//! application posts a notification. Read-only; we never see the message body
//! we don't parse — only the app name for a cute reaction line.
//!
//! If `dbus-monitor` isn't installed the feature silently disables itself.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

pub struct NotifyWatch {
    rx: Receiver<String>,
    child: Child,
}

impl NotifyWatch {
    pub fn spawn() -> Option<Self> {
        let mut child = Command::new("dbus-monitor")
            .arg("--session")
            .arg("interface='org.freedesktop.Notifications',member='Notify'")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let out = child.stdout.take()?;
        let (tx, rx) = channel();

        std::thread::spawn(move || {
            let reader = BufReader::new(out);
            let mut want_app = false;
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if line.contains("member=Notify") && line.contains("method call") {
                    // The first string argument after the call line is app_name.
                    want_app = true;
                    continue;
                }
                if want_app {
                    let t = line.trim();
                    if let Some(rest) = t.strip_prefix("string \"") {
                        let app = rest.trim_end_matches('"').to_string();
                        let _ = tx.send(app);
                        want_app = false;
                    } else if !t.starts_with("string") {
                        // unexpected shape — still report the event
                        let _ = tx.send(String::new());
                        want_app = false;
                    }
                }
            }
        });

        Some(Self { rx, child })
    }

    /// Non-blocking: the app name of a freshly posted notification, if any.
    pub fn try_event(&self) -> Option<String> {
        match self.rx.try_recv() {
            Ok(app) => Some(app),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

impl Drop for NotifyWatch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
