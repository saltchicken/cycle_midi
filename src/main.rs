mod ast;
mod engine;
mod io;
mod parser;

use ast::{Program, QuantizeMode};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup Config & Workspace
    let (app_config, watch_dir, file_path) = io::config::initialize_config();

    // 2. Setup Global Shutdown State
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\nReceived shutdown signal! Cleaning up MIDI notes...");
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    // 3. Setup AST Communication Channel - NOW PASSES (Filename, Program)
    let (tx, rx) = channel::<(String, Program)>();

    // 4. Start File Watcher Thread
    io::watcher::start_file_watcher(watch_dir, file_path, tx);

    // 5. Setup MIDI Out & I/O Thread
    let midi_tx = io::midi::setup_midi(&app_config.midi_port)?;

    // Parse the default quantize mode from config
    let global_quantize = match app_config.default_quantize.as_deref() {
        Some("auto") | Some("AUTO") => QuantizeMode::Auto,
        Some(num_str) => {
            if let Ok(num) = num_str.parse::<usize>() {
                QuantizeMode::Fixed(num)
            } else {
                eprintln!("Warning: Invalid default_quantize in config. Defaulting to 1.");
                QuantizeMode::Fixed(1)
            }
        }
        None => QuantizeMode::Fixed(1), // Ultimate fallback
    };

    // 6. Run the Main Real-Time Scheduler
    engine::scheduler::run_scheduler(rx, midi_tx, running, global_quantize);

    println!("Graceful shutdown complete.");
    Ok(())
}
