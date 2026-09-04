use crate::ast::Program;
use crate::parser::mmn_parser;
use chumsky::Parser;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::time::Duration;

pub fn start_file_watcher(watch_dir: PathBuf, file_path: PathBuf, tx: Sender<(String, Program)>) {
    thread::spawn(move || {
        let (watch_tx, watch_rx) = channel();

        let mut debouncer = new_debouncer(Duration::from_millis(150), watch_tx).unwrap();
        debouncer
            .watcher()
            .watch(&watch_dir, RecursiveMode::NonRecursive)
            .unwrap();

        println!(
            "Listening for changes to .mmn files in directory {}... (Starting with {})",
            watch_dir.display(),
            file_path.display()
        );

        let parser = mmn_parser();

        // Initially load the default file (e.g., live.mmn)
        if let Ok(contents) = fs::read_to_string(&file_path) {
            if let Ok(initial_prog) = parser.parse(contents) {
                let filename = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let _ = tx.send((filename, initial_prog));
            }
        }

        for res in watch_rx {
            match res {
                Ok(events) => {
                    // Find the most recently modified .mmn file in the event batch
                    let saved_mmn_file = events
                        .iter()
                        .find(|e| e.path.extension().and_then(|ext| ext.to_str()) == Some("mmn"))
                        .map(|e| &e.path);

                    if let Some(active_file) = saved_mmn_file {
                        let filename = active_file.file_name().unwrap_or_default().to_string_lossy().to_string();
                        println!("Detected save in: {}", active_file.display());
                        
                        if let Ok(contents) = fs::read_to_string(active_file) {
                            if contents.trim().is_empty() {
                                let empty_prog = Program {
                                    bpm: None,
                                    quantize: None,
                                    scale: None,
                                    global_silence: true,
                                    tracks: vec![],
                                };
                                if tx.send((filename.clone(), empty_prog)).is_ok() {
                                    println!("File empty. Silencing all tracks.");
                                }
                                continue;
                            }

                            match parser.parse(contents) {
                                Ok(new_prog) => {
                                    let _ = tx.send((filename, new_prog));
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
