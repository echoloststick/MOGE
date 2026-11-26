//! Run / Stop: executes the project's scripts with the Luau runtime (mlua) on a background thread.
use crate::net::Msg;
use crate::studio::{Level, Studio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

impl Studio {
    pub fn stop_run(&mut self) {
        if let Some(c) = &self.run_cancel {
            c.store(true, Ordering::Relaxed);
        }
    }

    #[cfg(feature = "luau")]
    pub fn start_run(&mut self) {
        if self.run_cancel.is_some() {
            return;
        }
        let scripts = moge_luau::collect_play_scripts(&self.ed.tree);
        if scripts.is_empty() {
            self.log(Level::Warn, "Nothing to run: the project has no enabled Script or LocalScript with source code");
            self.right = crate::studio::Right::Output;
            self.show_right = true;
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.run_cancel = Some(cancel.clone());
        self.right = crate::studio::Right::Output;
        self.show_right = true;
        self.log(Level::Info, format!("Running {} script(s) on a snapshot of the project", scripts.len()));
        let tree = self.ed.tree.clone();
        let dump = self.ed.dump.clone();
        let tx = self.net.sender();
        std::thread::spawn(move || {
            let tx_out = tx.clone();
            let res = moge_luau::run(tree, dump, scripts, cancel, None, move |lvl, text| {
                let l = match lvl {
                    moge_luau::Level::Warn => Level::Warn,
                    moge_luau::Level::Error => Level::Error,
                    _ => Level::Info,
                };
                let _ = tx_out.send(Msg::RunLine(l, text));
            });
            if let Err(e) = res {
                let _ = tx.send(Msg::RunLine(Level::Error, e));
            }
            let _ = tx.send(Msg::RunDone);
        });
    }

    #[cfg(not(feature = "luau"))]
    pub fn start_run(&mut self) {
        self.log(Level::Error, "This build does not include the Luau runtime (built without the 'luau' feature)");
    }
}
