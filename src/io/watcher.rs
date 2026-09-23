use crate::ast::{Program, Node};
use crate::parser::{mmn_parser, node_parser};
use chumsky::Parser;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::time::{Duration, SystemTime};

fn load_recursive(
    path: &Path,
    base_dir: &Path,
    parser: &impl Parser<char, Program, Error = chumsky::error::Simple<char>>,
    all_aliases: &mut HashMap<String, crate::ast::MacroDef>,
    visited: &mut HashSet<PathBuf>,
    cache: &mut HashMap<PathBuf, (SystemTime, Program)>,
    is_root: bool,
) -> Result<Option<Program>, String> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical.clone()) {
        return Ok(None); // Prevent infinite loops from circular includes
    }

    let modified_time = fs::metadata(&canonical)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut prog_opt = None;
    if let Some((cached_time, cached_prog)) = cache.get(&canonical) {
        if *cached_time == modified_time {
            prog_opt = Some(cached_prog.clone());
        }
    }

    let prog = match prog_opt {
        Some(p) => p,
        None => {
            let content = fs::read_to_string(&canonical)
                .map_err(|e| format!("Could not read file {}: {}", path.display(), e))?;

            match parser.parse(content) {
                Ok(parsed) => {
                    cache.insert(canonical.clone(), (modified_time, parsed.clone()));
                    parsed
                }
                Err(errs) => {
                    println!("Syntax Error in {}!", path.display());
                    for e in errs {
                        let expected: Vec<_> = e.expected().cloned().collect();
                        eprintln!("Expected {:?} at char {}", expected, e.span().start);
                    }
                    return Err("Syntax error".to_string());
                }
            }
        }
    };

    for inc in &prog.includes {
        let inc_path = base_dir.join(inc);
        load_recursive(
            &inc_path,
            base_dir,
            parser,
            all_aliases,
            visited,
            cache,
            false,
        )?;
    }

    for (k, v) in prog.aliases.clone() {
        all_aliases.insert(k, v);
    }

    if is_root {
        Ok(Some(prog))
    } else {
        Ok(None)
    }
}

fn resolve_midi_nodes(
    node: &mut Node,
    base_dir: &Path,
    node_parser: &impl Parser<char, Node, Error = chumsky::error::Simple<char>>,
) -> Result<(), String> {
    match node {
        Node::MidiImport(options) => {
            // Execute the pure rust midi conversion, passing the resulting text directly into our node_parser
            let mut parsed_node = super::midi_convert::convert_midi_to_node(options, base_dir, node_parser)?;
            
            // Just in case the parsed structure itself had weird nested macros or imports (unlikely for MIDI, but safe)
            resolve_midi_nodes(&mut parsed_node, base_dir, node_parser)?;
            *node = parsed_node;
        }
        Node::Sequence(seq) | Node::Chord(seq) | Node::ShuffledSequence(seq) | Node::Alternator(seq) => {
            for n in seq { resolve_midi_nodes(n, base_dir, node_parser)?; }
        }
        Node::Parallel(layers) | Node::Polymeter(layers) => {
            for layer in layers {
                for n in layer { resolve_midi_nodes(n, base_dir, node_parser)?; }
            }
        }
        Node::Arrange(segments) => {
            for (_, _, child) in segments { resolve_midi_nodes(child, base_dir, node_parser)?; }
        }
        Node::WithScale(_, child) | Node::Struct(_, child) | Node::Modified(child, _) => {
            resolve_midi_nodes(child, base_dir, node_parser)?;
        }
        Node::RandomChoice(choices) => {
            for (_, child) in choices { resolve_midi_nodes(child, base_dir, node_parser)?; }
        }
        Node::Ref(_, args) => {
            for arg in args { resolve_midi_nodes(arg, base_dir, node_parser)?; }
        }
        _ => {}
    }
    Ok(())
}

pub fn start_file_watcher(watch_dir: PathBuf, file_path: PathBuf, tx: Sender<(String, Program)>) {
    thread::spawn(move || {
        let (watch_tx, watch_rx) = channel();

        let mut debouncer = match new_debouncer(Duration::from_millis(150), watch_tx) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Failed to create file watcher debouncer: {}", e);
                return;
            }
        };

        if let Err(e) = debouncer
            .watcher()
            .watch(&watch_dir, RecursiveMode::NonRecursive)
        {
            eprintln!("Failed to watch directory {}: {}", watch_dir.display(), e);
            return;
        }

        println!(
            "Listening for changes to .mmn files in directory {}... (Starting with {})",
            watch_dir.display(),
            file_path.display()
        );

        let parser = mmn_parser();
        let n_parser = node_parser();
        let mut ast_cache: HashMap<PathBuf, (SystemTime, Program)> = HashMap::new();

        let process_file = |active_file_path: &Path,
                            cache: &mut HashMap<PathBuf, (SystemTime, Program)>|
         -> Result<Program, String> {
            let mut all_aliases = HashMap::new();
            let mut visited = HashSet::new();

            match load_recursive(
                active_file_path,
                &watch_dir,
                &parser,
                &mut all_aliases,
                &mut visited,
                cache,
                true,
            )? {
                Some(mut main_prog) => {
                    main_prog.aliases = all_aliases;

                    // 1. Resolve and replace all physical MIDI files in tracks
                    for track in &mut main_prog.tracks {
                        resolve_midi_nodes(&mut track.root_node, &watch_dir, &n_parser)?;
                    }
                    
                    // ... and in macros (so `let MELODY = song1.midi` works perfectly)
                    for macro_def in main_prog.aliases.values_mut() {
                        resolve_midi_nodes(&mut macro_def.body, &watch_dir, &n_parser)?;
                    }

                    // 2. Expand all standard macros now that the raw midi structures are injected
                    main_prog.expand_all_refs().map(|_| main_prog)
                }
                None => Err("Failed to load root file".to_string()),
            }
        };

        if file_path.exists() {
            if let Ok(initial_prog) = process_file(&file_path, &mut ast_cache) {
                let filename = file_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let _ = tx.send((filename, initial_prog));
            }
        }

        for res in watch_rx {
            match res {
                Ok(events) => {
                    let saved_mmn_file = events
                        .iter()
                        .find(|e| e.path.extension().and_then(|ext| ext.to_str()) == Some("mmn"))
                        .map(|e| &e.path);

                    if let Some(active_file) = saved_mmn_file {
                        let filename = active_file
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        println!("Detected save in: {}", active_file.display());

                        if let Ok(contents) = fs::read_to_string(active_file) {
                            if contents.trim().is_empty() {
                                let empty_prog = Program {
                                    bpm: None,
                                    signature: None,
                                    quantize: None,
                                    scale: None,
                                    scale_seq: None,
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

                        match process_file(active_file, &mut ast_cache) {
                            Ok(new_prog) => {
                                let _ = tx.send((filename, new_prog));
                            }
                            Err(e) => {
                                println!(
                                    "Compilation Error! Continuing to play old sequence. ({})",
                                    e
                                );
                            }
                        }
                    }
                }
                Err(e) => eprintln!("File Watcher Error: {:?}", e),
            }
        }
    });
}
