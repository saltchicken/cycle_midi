mod ast;
mod parser;
mod render;

use ast::{Node, Program, ScheduledNote};
use parser::mmn_parser;
use render::generate_next_cycle;

use chumsky::Parser;
use midir::MidiOutput;
use midir::os::unix::VirtualOutput; 
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::Path;
use std::sync::mpsc::{channel, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = channel::<Program>();
    let file_path = "live.mmn";

    // Setup initial file with a default BPM tag
    if !Path::new(file_path).exists() {
        fs::write(file_path, "#BPM=120\nC4 . D4 _").expect("Failed to create initial file");
    }

    // Spawn Watcher Thread
    thread::spawn(move || {
        let (watch_tx, watch_rx) = channel();
        let mut watcher = RecommendedWatcher::new(
            watch_tx,
            Config::default().with_poll_interval(Duration::from_millis(50)),
        ).unwrap();

        let parent_dir = Path::new(file_path).parent().unwrap_or(Path::new(""));
        let watch_dir = if parent_dir.as_os_str().is_empty() { Path::new(".") } else { parent_dir };
        
        watcher.watch(watch_dir, RecursiveMode::NonRecursive).unwrap();
        println!("Listening for changes to {} in directory {:?}...", file_path, watch_dir);

        if let Ok(contents) = fs::read_to_string(file_path) {
             if let Ok(initial_prog) = mmn_parser().parse(contents) {
                 let _ = tx.send(initial_prog);
             }
        }

        let mut last_update = Instant::now() - Duration::from_secs(1);

        for res in watch_rx {
            if let Ok(event) = res {
                let is_target_file = event.paths.iter().any(|p| p.ends_with(file_path));
                
                if is_target_file && !event.kind.is_access() {
                    if last_update.elapsed() < Duration::from_millis(100) {
                        continue;
                    }

                    thread::sleep(Duration::from_millis(15));
                    
                    if let Ok(contents) = fs::read_to_string(file_path) {
                        if contents.trim().is_empty() { continue; }

                        match mmn_parser().parse(contents) {
                            Ok(new_prog) => {
                                if tx.send(new_prog).is_ok() {
                                    println!("Success! AST hot-swapped.");
                                    last_update = Instant::now();
                                }
                            }
                            Err(errs) => {
                                println!("Syntax Error! Continuing to play old sequence.");
                                for e in errs {
                                    let expected: Vec<_> = e.expected().cloned().collect();
                                    eprintln!("Expected {:?} at char {}", expected, e.span().start);
                                }
                                last_update = Instant::now();
                            }
                        }
                    }
                }
            }
        }
    });

    // MIDI Output Setup
    let midi_out = MidiOutput::new("Cycle MIDI Scheduler")?;
    let mut conn_out = midi_out.create_virtual("MMN Live Port")?;
    println!("Virtual MIDI Port 'MMN Live Port' created. Route it to your synth!");

    // Scheduler State
    let mut bpm = 120.0;
    let mut cycle_duration_ms = (60_000.0 / bpm) * 4.0;
    let mut current_ast = Node::Sequence(vec![]);
    let mut cycle_count = 0;
    
    let start_time = Instant::now();
    let mut next_cycle_start_ms = 0.0;
    
    let mut upcoming_notes: Vec<ScheduledNote> = Vec::new();
    let mut active_notes: Vec<(f64, u8)> = Vec::new();

    println!("Starting Scheduler Loop...");

    // The Real-Time Loop
    loop {
        let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        // 1. Hot-Swap AST and potentially adjust BPM
        match rx.try_recv() {
            Ok(new_prog) => {
                current_ast = new_prog.root_node;
                if let Some(new_bpm) = new_prog.bpm {
                    if (new_bpm - bpm).abs() > f64::EPSILON {
                        bpm = new_bpm;
                        cycle_duration_ms = (60_000.0 / bpm) * 4.0;
                        println!("BPM updated to: {}", bpm);
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }

        // 2. Generate Next Cycle if we reached the boundary
        if elapsed_ms >= next_cycle_start_ms {
            let mut new_notes = generate_next_cycle(
                &current_ast,
                bpm,
                next_cycle_start_ms,
                cycle_count,
            );
            
            new_notes.sort_by(|a, b| b.start_ms.partial_cmp(&a.start_ms).unwrap());
            upcoming_notes = new_notes;
            
            next_cycle_start_ms += cycle_duration_ms;
            cycle_count += 1;
        }

        // 3. Process Note Offs
        active_notes.retain(|&(off_time, pitch)| {
            if elapsed_ms >= off_time {
                let _ = conn_out.send(&[0x80, pitch, 0]);
                false
            } else {
                true
            }
        });

        // 4. Process Note Ons
        while let Some(next_note) = upcoming_notes.last() {
            if elapsed_ms >= next_note.start_ms {
                let note = upcoming_notes.pop().unwrap();
                let _ = conn_out.send(&[0x90, note.pitch, note.velocity]);
                active_notes.push((note.start_ms + note.duration_ms, note.pitch));
            } else {
                break;
            }
        }

        thread::sleep(Duration::from_millis(1)); 
    }

    Ok(())
}
