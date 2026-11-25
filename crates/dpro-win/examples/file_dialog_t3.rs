//! G1's picker dialog check (TD-0009 A3.5 (12), A3.9, slice 6e): the system's file dialog, modal
//! to a `dpro_win::gui::Window`, asked for the way the GUI asks. The handler asks with
//! `WindowCx::pick` while it handles an event, the window shows the dialog once the handler has
//! returned, and the answer comes back through `Handler::picked`. `.github/scripts/ui-gates.ps1`
//! ("Composites: picker dialog") finds the dialog through UI Automation, cancels it, and reads
//! this probe's report. A dev tool, never shipped.
//!
//! ```text
//! file_dialog_t3 [--mode folder|file] [--start <dir>] --out <report>
//! ```
//!
//! The window, "DISM Pro file dialog probe", asks 300 ms after it's created, titling the dialog
//! "DISM Pro probe: choose a folder" (or "a file", with two file types). While the dialog is up
//! the window wakes itself every 50 ms and repaints, so the report says how many timer messages
//! and paints the dialog's modal loop dispatched to it. Its UI Automation tree is a Window with
//! one Button whose name says where the pick is (`pick: waiting`, `pick: asked`, then the
//! answer), so a client can read it while the dialog is up and after it closed: the thread is in
//! a single-threaded apartment only while the dialog is up (TD-0009 §5.3 as A3.5 (12) amends it).
//!
//! The report, written when the answer comes, one `key=value` a line: `picked=` `cancelled`,
//! `path`, `unavailable` or `failed`; `ticks=` and `paints=`, what the window heard while the
//! dialog was up; `code=` and `api=` for an error; `path=` last, for a path. The window stays
//! open until it's closed (`WM_CLOSE`), so a client can read its tree after the dialog.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(windows)]
    {
        probe::main()
    }
    #[cfg(not(windows))]
    {
        ExitCode::FAILURE
    }
}

#[cfg(windows)]
mod probe {
    use std::{
        fmt::Write as _,
        fs,
        path::{Path, PathBuf},
        process::ExitCode,
    };

    use dpro_win::gui::{
        Compose, Event, Frame, Handler, RectPx, SizePx, Surface, Window, WindowCx, WindowOptions,
        accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate},
        file_dialog::{Pick, PickError, PickMode, Picked},
    };

    const TITLE: &str = "DISM Pro file dialog probe";
    const FOLDER_TITLE: &str = "DISM Pro probe: choose a folder";
    const FILE_TITLE: &str = "DISM Pro probe: choose a file";
    const TYPES: [(&str, &str); 2] = [("Windows images", "*.wim;*.esd"), ("All files", "*.*")];
    /// From the window's creation to the request: it shows first.
    const ASK_MS: u32 = 300;
    /// The tick while the dialog is up: each one a timer message the modal loop dispatches.
    const TICK_MS: u32 = 50;
    /// The window's light fill (`Tone::App`), premultiplied BGRA.
    const FILL: u32 = 0xFFF6_F7F9;
    const ROOT: NodeId = NodeId(0);
    const STATUS: NodeId = NodeId(1);

    struct Args {
        file: bool,
        start: Option<PathBuf>,
        out: PathBuf,
    }

    impl Args {
        fn parse() -> Result<Self, String> {
            let (mut file, mut start, mut out) = (false, None, None);
            let mut args = std::env::args_os().skip(1);
            while let Some(arg) = args.next() {
                let arg = arg.to_string_lossy().into_owned();
                let value = args.next().ok_or_else(|| format!("{arg} needs a value"))?;
                match arg.as_str() {
                    "--mode" => {
                        file = match value.to_string_lossy().as_ref() {
                            "folder" => false,
                            "file" => true,
                            other => {
                                return Err(format!("--mode {other}: expected folder or file"));
                            }
                        };
                    }
                    "--start" => start = Some(PathBuf::from(value)),
                    "--out" => out = Some(PathBuf::from(value)),
                    _ => return Err(format!("unknown argument {arg}")),
                }
            }
            Ok(Self {
                file,
                start,
                out: out.ok_or("--out <report> is required")?,
            })
        }
    }

    /// Where the pick is.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Stage {
        Waiting,
        Asked,
        Answered(&'static str),
    }

    struct Probe {
        args: Args,
        stage: Stage,
        armed: bool,
        size: SizePx,
        /// Wakes and paints while the dialog was up.
        ticks: u32,
        paints: u32,
        /// The tree changed since UI Automation last had it.
        changed: bool,
    }

    impl Probe {
        fn ask(&mut self, cx: &mut WindowCx<'_>) {
            let mode = if self.args.file {
                PickMode::File { types: &TYPES }
            } else {
                PickMode::Folder
            };
            let pick = Pick {
                mode,
                title: if self.args.file {
                    FILE_TITLE
                } else {
                    FOLDER_TITLE
                },
                start: self.args.start.as_deref(),
            };
            if cx.pick(&pick) {
                self.stage = Stage::Asked;
                self.changed = true;
                cx.wake_after(Some(TICK_MS));
            }
        }

        fn whole(&self) -> RectPx {
            RectPx {
                x0: 0,
                y0: 0,
                x1: i32::try_from(self.size.w).unwrap_or(i32::MAX),
                y1: i32::try_from(self.size.h).unwrap_or(i32::MAX),
            }
        }

        fn tree(&self) -> TreeUpdate {
            let mut root = Node::new(Role::Window);
            root.set_label(TITLE);
            root.set_children(vec![STATUS]);
            let mut status = Node::new(Role::Button);
            status.set_label(match self.stage {
                Stage::Waiting => "pick: waiting",
                Stage::Asked => "pick: asked",
                Stage::Answered(answer) => answer,
            });
            TreeUpdate {
                nodes: vec![(ROOT, root), (STATUS, status)],
                tree: Some(TreeInfo::new(ROOT)),
                tree_id: TreeId::ROOT,
                focus: ROOT,
            }
        }

        /// The report, as the module's doc says.
        fn report(&self, picked: &Picked) -> String {
            let (answer, error) = match picked {
                Ok(None) => ("cancelled", None),
                Ok(Some(_)) => ("path", None),
                Err(PickError::Unavailable(e)) => ("unavailable", Some(e)),
                Err(PickError::Failed(e)) => ("failed", Some(e)),
            };
            let mut out = format!(
                "picked={answer}\nticks={}\npaints={}\n",
                self.ticks, self.paints
            );
            // ignore: writing to a String can't fail.
            if let Some(e) = error {
                let _ = writeln!(out, "code={:#010x}\napi={}", e.hresult_bits(), e.api);
            }
            if let Ok(Some(path)) = picked {
                let _ = writeln!(out, "path={}", path.display());
            }
            out
        }
    }

    impl Handler for Probe {
        fn event(&mut self, event: Event, cx: &mut WindowCx<'_>) -> bool {
            match event {
                Event::Resize(size) => {
                    self.size = size;
                    if !self.armed {
                        self.armed = true;
                        cx.wake_after(Some(ASK_MS));
                    }
                    cx.invalidate(self.whole());
                }
                Event::Wake => match self.stage {
                    Stage::Waiting => self.ask(cx),
                    Stage::Asked => {
                        // Heard through the dialog's modal loop.
                        self.ticks += 1;
                        cx.invalidate(self.whole());
                        cx.wake_after(Some(TICK_MS));
                    }
                    Stage::Answered(_) => {}
                },
                Event::Close => cx.close(),
                _ => return false,
            }
            true
        }

        fn paint(&mut self, frame: &mut Frame<'_>) {
            frame.pixels.fill(FILL);
            if self.stage == Stage::Asked {
                self.paints += 1;
            }
        }

        fn a11y_tree(&mut self, _: Surface) -> Option<TreeUpdate> {
            self.changed = false;
            Some(self.tree())
        }

        fn a11y_update(&mut self, _: Surface) -> Option<TreeUpdate> {
            std::mem::take(&mut self.changed).then(|| self.tree())
        }

        fn picked(&mut self, picked: Picked, cx: &mut WindowCx<'_>) {
            cx.wake_after(None);
            let report = self.report(&picked);
            self.stage = Stage::Answered(match picked {
                Ok(None) => "pick: cancelled",
                Ok(Some(_)) => "pick: a path",
                Err(PickError::Unavailable(_)) => "pick: unavailable",
                Err(PickError::Failed(_)) => "pick: failed",
            });
            self.changed = true;
            // ignore: an unwritable report shows as none, which G1 reports.
            let _ = write(&self.args.out, &report);
        }
    }

    /// Writes the report whole: G1 reads it as soon as it exists.
    fn write(out: &Path, report: &str) -> std::io::Result<()> {
        let part = out.with_extension("part");
        fs::write(&part, report)?;
        fs::rename(&part, out)
    }

    pub(crate) fn main() -> ExitCode {
        let Ok(args) = Args::parse() else {
            return ExitCode::from(2);
        };
        // ignore: no earlier report is the usual case; G1 waits for this run's.
        let _ = fs::remove_file(&args.out);
        let probe = Probe {
            args,
            stage: Stage::Waiting,
            armed: false,
            size: SizePx::default(),
            ticks: 0,
            paints: 0,
            changed: false,
        };
        let options = WindowOptions {
            title: TITLE,
            size: (640, 480),
            min_size: (320, 240),
            compose: Compose::Never,
        };
        let Ok(window) = Window::new(&options, Box::new(probe)) else {
            return ExitCode::FAILURE;
        };
        window.show();
        match window.run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        }
    }
}
