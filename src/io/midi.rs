use midir::MidiOutput;
use midir::os::unix::VirtualOutput;
use rtrb::{Producer, RingBuffer};
use std::thread;
use std::time::Duration;
use thread_priority::*;

pub fn setup_midi(
    target_port: &Option<String>,
) -> Result<Producer<Vec<u8>>, Box<dyn std::error::Error>> {
    let mut midi_out = MidiOutput::new("Cycle MIDI Scheduler")?;

    let mut conn_out = 'setup: {
        if let Some(target_port) = target_port {
            let ports = midi_out.ports();
            let mut found_port = None;
            for p in &ports {
                if let Ok(name) = midi_out.port_name(p) {
                    if name.to_lowercase().contains(&target_port.to_lowercase()) {
                        found_port = Some(p.clone());
                        println!("Found matching MIDI port: {}", name);
                        break;
                    }
                }
            }

            if let Some(p) = found_port {
                match midi_out.connect(&p, "Cycle MIDI Out") {
                    Ok(conn) => {
                        println!("Successfully connected to designated MIDI port!");
                        break 'setup conn;
                    }
                    Err(e) => {
                        eprintln!("Failed to connect to MIDI port: {}", e);
                        midi_out = e.into_inner();
                    }
                }
            } else {
                println!("MIDI port '{}' not found in available ports.", target_port);
            }
        }

        println!("Falling back to Virtual MIDI Port.");
        let conn = midi_out.create_virtual("MMN Live Port")?;
        println!("Virtual MIDI Port 'MMN Live Port' created. Route it to your synth!");
        conn
    };

    let (midi_tx, mut midi_rx) = RingBuffer::<Vec<u8>>::new(4096);

    thread::spawn(move || {
        let thread_id = thread_native_id();
        let _ = set_thread_priority_and_policy(
            thread_id,
            ThreadPriority::Max,
            ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
        );

        let mut shutdown = false;
        loop {
            if midi_rx.is_empty() {
                if shutdown {
                    break;
                }
                thread::sleep(Duration::from_micros(500));
                continue;
            }

            while let Ok(msg) = midi_rx.pop() {
                if msg.is_empty() {
                    shutdown = true;
                    break;
                }
                let _ = conn_out.send(&msg);
            }
        }
    });

    Ok(midi_tx)
}
