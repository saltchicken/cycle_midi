use crate::ast::Program;
use crate::parser::mmn_parser;
use chumsky::Parser;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::time::Duration;

pub fn start_file_watcher(watch_dir: PathBuf, file_path: PathBuf, tx: Sender<Program>) {
    thread::spawn(move || {
        let (watch_tx, watch_rx) = channel();

        let mut debouncer = new_debouncer(Duration::from_millis(150), watch_tx).unwrap();
        debouncer
            .watcher()
            .watch(&watch_dir, RecursiveMode::NonRecursive)
            .unwrap();

        println!(
            "Listening for changes to {} in directory {}...",
            file_path.display(),
            watch_dir.display()
        );

        let parser = mmn_parser();

        if let Ok(contents) = fs::read_to_string(&file_path) {
            if let Ok(initial_prog) = parser.parse(contents) {
                let _ = tx.send(initial_prog);
            }
        }

        for res in watch_rx {
            match res {
                Ok(events) => {
                    let is_target_file = events.iter().any(|e| e.path == file_path);

                    if is_target_file {
                        if let Ok(contents) = fs::read_to_string(&file_path) {
                            if contents.trim().is_empty() {
                                let empty_prog = Program {
                                    bpm: None,
                                    quantize: None,
                                    scale: None,
                                    global_silence: true,
                                    tracks: vec![],
                                };
                                if tx.send(empty_prog).is_ok() {
                                    println!("File empty. Silencing all tracks.");
                                }
                                continue;
                            }

                            match parser.parse(contents) {
                                Ok(new_prog) => {
                                    let _ = tx.send(new_prog);
                                }
                                Err(errs) => {
                                    println!("Syntax Error! Continuing to play old sequence.");
                                    for e in errs {
                                        let expected: Vec<_> = e.expected().cloned().collect();
                                        eprintln!(
                                            "Expected {:?} at char {}",
                                            expected,
                                            e.span().start
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => eprintln!("File Watcher Error: {:?}", e),
            }
        }
    });
}
