import mido

# Config
MIDI_FILE = "PreludeCMajor.mid"
CYCLES_PER_CHUNK = 2  # Matches your (0, 2), (2, 4) windowing
STEPS_PER_CYCLE = 4   # Assuming 1 cycle = 4 beats = 4 steps here

def midi_to_note_name(midi_note):
    notes = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
    octave = (midi_note // 12) - 1
    note = notes[midi_note % 12]
    return f"{note}{octave}"

def convert_midi_to_mmn(filepath):
    mid = mido.MidiFile(filepath)
    ticks_per_beat = mid.ticks_per_beat
    
    # We'll extract all Note On events with their absolute tick timing
    notes = []
    absolute_ticks = 0
    
    for msg in mid.tracks[0]: # Assuming melody is on track 0
        absolute_ticks += msg.time
        if msg.type == 'note_on' and msg.velocity > 0:
            notes.append({
                'pitch': midi_to_note_name(msg.note),
                'ticks': absolute_ticks
            })

    # Group notes by your macro-cycles
    # 1 cycle = 4 beats (assuming 4/4 time). 
    ticks_per_cycle = ticks_per_beat * 4 
    ticks_per_chunk = ticks_per_cycle * CYCLES_PER_CHUNK

    chunks = {}
    for note in notes:
        chunk_idx = int(note['ticks'] // ticks_per_chunk)
        if chunk_idx not in chunks:
            chunks[chunk_idx] = []
        chunks[chunk_idx].append(note['pitch'])

    # Format into your AST syntax
    print("T1:\n  seqPLoop {")
    
    max_chunk = max(chunks.keys()) if chunks else 0
    for i in range(max_chunk + 1):
        start_cycle = i * CYCLES_PER_CHUNK
        end_cycle = start_cycle + CYCLES_PER_CHUNK
        
        sequence = chunks.get(i, ["."]) # Use your AST's rest '.' if empty
        seq_str = " ".join(sequence)
        
        # Format: (0, 2): [C4 E4 G4 ...] |
        separator = "|" if i < max_chunk else ""
        print(f"    ({start_cycle}, {end_cycle}): [{seq_str}]    {separator}")
        
    print("  }")

if __name__ == "__main__":
    convert_midi_to_mmn(MIDI_FILE)
