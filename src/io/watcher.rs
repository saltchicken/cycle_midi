use crate::ast::Program;
use crate::parser::mmn_parser;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
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
        let watch_dir_clone = watch_dir.clone();

        // Helper closure to recursively load files, process includes, and expand references
        let process_file = |active_file_path: &Path| -> Result<Program, String> {
            let mut all_aliases = HashMap::new();
            let mut visited = HashSet::new();

            fn load_recursive(
                path: &Path,
                base_dir: &Path,
                parser: &impl chumsky::Parser<char, Program, Error = chumsky::error::Simple<char>>,
                all_aliases: &mut HashMap<String, crate::ast::Node>,
                visited: &mut HashSet<PathBuf>,
                is_root: bool,
            ) -> Result<Option<Program>, String> {
                let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                if !visited.insert(canonical.clone()) {
                    return Ok(None); // Prevent infinite loops from circular includes
                }

                let content = fs::read_to_string(&canonical)
                    .map_err(|e| format!("Could not read file {}: {}", path.display(), e))?;

                match parser.parse(content) {
                    Ok(mut prog) => {
                        // Recursively load includes first
                        for inc in &prog.includes {
                            let inc_path = base_dir.join(inc);
                            load_recursive(&inc_path, base_dir, parser, all_aliases, visited, false)?;
                        }

                        // Merge this file's aliases into the master list
                        for (k, v) in prog.aliases.drain() {
                            all_aliases.insert(k, v);
                        }

                        if is_root {
                            Ok(Some(prog))
                        } else {
                            Ok(None) // We only care about aliases for included files
                        }
                    }
                    Err(errs) => {
                        println!("Syntax Error in {}!", path.display());
                        for e in errs {
                            let expected: Vec<_> = e.expected().cloned().collect();
                            eprintln!(
                                "Expected {:?} at char {}",
                                expected,
                                e.span().start
                            );
                        }
                        Err("Syntax error".to_string())
                    }
                }
            }

            match load_recursive(active_file_path, &watch_dir_clone, &parser, &mut all_aliases, &mut visited, true)? {
                Some(mut main_prog) => {
                    main_prog.aliases = all_aliases;
                    main_prog.expand_all_refs().map(|_| main_prog)
                }
                None => Err("Failed to load root file".to_string()),
            }
        };

        // Initially load the default file (e.g., live.mmn)
        if file_path.exists() {
            if let Ok(initial_prog) = process_file(&file_path) {
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
                            // If the user clears the file entirely, immediately silence the sequence
                            if contents.trim().is_empty() {
                                let empty_prog = Program {
                                    bpm: None,
                                    quantize: None,
                                    scale: None,
                                    global_silence: true,
                                    includes: vec![],
                                    aliases: HashMap::new(),
                                    tracks: vec![],
                                };
                                if tx.send((filename.clone(), empty_prog)).is_ok() {
                                    println!("File empty. Silencing all tracks.");
                                }
                                continue;
                            }
                        }

                        match process_file(active_file) {
                            Ok(new_prog) => {
                                let _ = tx.send((filename, new_prog));
                            }
                            Err(e) => {
                                println!("Compilation Error! Continuing to play old sequence. ({})", e);
                            }
                        }
                    }
                }
                Err(e) => eprintln!("File Watcher Error: {:?}", e),
            }
        }
    });
}
