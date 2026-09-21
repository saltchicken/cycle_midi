import mido
import re
import argparse

SCALES = {
    "major": [0, 2, 4, 5, 7, 9, 11],
    "minor": [0, 2, 3, 5, 7, 8, 10],
    "minor_pentatonic": [0, 3, 5, 7, 10],
    "dorian": [0, 2, 3, 5, 7, 9, 10],
    "phrygian": [0, 1, 3, 5, 7, 8, 10],
    "lydian": [0, 2, 4, 6, 7, 9, 11],
    "mixolydian": [0, 2, 4, 5, 7, 9, 10],
    "locrian": [0, 1, 3, 5, 6, 8, 10],
    "pentatonic": [0, 2, 4, 7, 9],
    "chromatic": [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
}

def parse_root_pitch(note_name):
    notes = {"C": 0, "C#": 1, "DB": 1, "D": 2, "D#": 3, "EB": 3, 
             "E": 4, "F": 5, "F#": 6, "GB": 6, "G": 7, "G#": 8, 
             "AB": 8, "A": 9, "A#": 10, "BB": 10, "B": 11}
    
    match = re.match(r"([A-Ga-g][#bB]?)(-?\d+)", note_name)
    if not match:
        return 60
        
    note_str = match.group(1).upper()
    octave = int(match.group(2))
    
    base = notes.get(note_str, 0)
    return (octave + 1) * 12 + base

def midi_to_scale_degree(midi_note, root_midi, scale_intervals):
    diff = midi_note - root_midi
    octave_diff = diff // 12
    pc_diff = diff % 12
    scale_len = len(scale_intervals)
    
    if pc_diff in scale_intervals:
        degree_index = scale_intervals.index(pc_diff)
        numeric_degree = (octave_diff * scale_len) + degree_index
        return str(numeric_degree)
        
    closest_idx = 0
    for i, interval in enumerate(scale_intervals):
        if interval < pc_diff:
            closest_idx = i
            
    numeric_degree = (octave_diff * scale_len) + closest_idx
    accidental = pc_diff - scale_intervals[closest_idx]
    return f"{numeric_degree}{'#' * accidental}"

def extract_quantized_grid(mid, root_midi, scale_intervals, steps_per_cycle, beats_per_cycle):
    ticks_per_beat = mid.ticks_per_beat
    ticks_per_cycle = ticks_per_beat * beats_per_cycle 
    step_ticks = ticks_per_cycle / steps_per_cycle

    note_spans = []
    for track in mid.tracks:
        abs_tick = 0
        active_notes = {}
        
        for msg in track:
            abs_tick += msg.time
            if msg.type == 'note_on' and msg.velocity > 0:
                active_notes[msg.note] = abs_tick
            elif msg.type == 'note_off' or (msg.type == 'note_on' and msg.velocity == 0):
                if msg.note in active_notes:
                    start_tick = active_notes.pop(msg.note)
                    degree_str = midi_to_scale_degree(msg.note, root_midi, scale_intervals)
                    note_spans.append((start_tick, abs_tick, degree_str))

    if not note_spans:
        return []

    max_tick = max(span[1] for span in note_spans)
    total_steps = int((max_tick + step_ticks - 1) // step_ticks)
    
    if total_steps % steps_per_cycle != 0:
        total_steps += steps_per_cycle - (total_steps % steps_per_cycle)
        
    grid = []
    
    for step in range(total_steps):
        window_start = step * step_ticks
        window_end = (step + 1) * step_ticks
        
        starting_notes = [n for n in note_spans if window_start <= n[0] < window_end]
        
        if starting_notes:
            grid.append(starting_notes[0][2])
        else:
            overlap_threshold = window_start + (step_ticks * 0.25)
            holding_notes = [n for n in note_spans if n[0] < window_start and n[1] > overlap_threshold]
            
            if holding_notes:
                grid.append("_")
            else:
                grid.append(".")
                
    return grid

def convert_midi_to_intervals(filepath, target_scale, output_format, steps_per_cycle, beats_per_cycle):
    try:
        mid = mido.MidiFile(filepath)
    except Exception as e:
        print(f"Error loading MIDI file: {e}")
        return

    parts = target_scale.split()
    root_name = parts[0]
    scale_name = parts[1] if len(parts) > 1 else "major"
    
    root_midi = parse_root_pitch(root_name)
    scale_intervals = SCALES.get(scale_name.lower(), SCALES["major"])
    
    grid = extract_quantized_grid(mid, root_midi, scale_intervals, steps_per_cycle, beats_per_cycle)

    if not grid:
        print("No Note On events found in the MIDI file.")
        return

    if output_format == "raw":
        print(" ".join(grid))
        
    elif output_format == "seqploop":
        print("seqPLoop {")
        
        chunks = [grid[i:i + steps_per_cycle] for i in range(0, len(grid), steps_per_cycle)]
        max_chunk = len(chunks) - 1
        
        for i, chunk in enumerate(chunks):
            seq_str = " ".join(chunk)
            separator = " |" if i < max_chunk else ""
            print(f"  ({i}, {i + 1}): [{seq_str}]{separator}")
            
        print("}")

    elif output_format == "chain":
        print("chain {")
        
        chunks = [grid[i:i + steps_per_cycle] for i in range(0, len(grid), steps_per_cycle)]
        
        if chunks:
            # Run-length encode the chunks to take advantage of chain's relative durations
            rle_chunks = []
            current_chunk = chunks[0]
            count = 1
            
            for chunk in chunks[1:]:
                if chunk == current_chunk:
                    count += 1
                else:
                    rle_chunks.append((count, current_chunk))
                    current_chunk = chunk
                    count = 1
            rle_chunks.append((count, current_chunk))
            
            for count, chunk in rle_chunks:
                seq_str = " ".join(chunk)
                print(f"  {count}: [{seq_str}]")
            
        print("}")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Quantize a MIDI file into MMN sequence syntax.")
    
    parser.add_argument(
        "filepath", 
        type=str, 
        help="Path to the input .mid file"
    )
    parser.add_argument(
        "-s", "--scale", 
        type=str, 
        default="C4 major", 
        help="Target scale (e.g., 'C4 major', 'G3 minor'). Default: 'C4 major'"
    )
    parser.add_argument(
        "-f", "--format", 
        type=str, 
        choices=["raw", "seqploop", "chain"], 
        default="chain", 
        help="Output format. 'raw' for flat list, 'seqploop' for chunked cycles, 'chain' for relative duration sequence. Default: 'chain'"
    )
    parser.add_argument(
        "-g", "--grid", 
        type=int, 
        default=8, 
        help="Grid resolution (steps per cycle). Default: 8"
    )
    parser.add_argument(
        "-b", "--beats", 
        type=float, 
        default=2.0, 
        help="Number of beats that make up a single macro-cycle. Default: 2.0"
    )

    args = parser.parse_args()

    convert_midi_to_intervals(
        filepath=args.filepath, 
        target_scale=args.scale, 
        output_format=args.format, 
        steps_per_cycle=args.grid,
        beats_per_cycle=args.beats
    )
