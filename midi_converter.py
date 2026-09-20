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

def convert_midi_to_intervals(filepath, target_scale, output_format, notes_per_cycle):
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
    
    notes = []
    for track in mid.tracks:
        for msg in track:
            if msg.type == 'note_on' and msg.velocity > 0:
                degree_str = midi_to_scale_degree(msg.note, root_midi, scale_intervals)
                notes.append(degree_str)

    if not notes:
        print("No Note On events found in the MIDI file.")
        return

    # Render Output
    print(f"#SCALE={target_scale}")
    print("T1:")
    
    if output_format == "raw":
        print("  " + " ".join(notes))
        
    elif output_format == "seqploop":
        print("  seqPLoop {")
        
        # Split notes into chunks of 'notes_per_cycle'
        chunks = [notes[i:i + notes_per_cycle] for i in range(0, len(notes), notes_per_cycle)]
        max_chunk = len(chunks) - 1
        
        for i, chunk in enumerate(chunks):
            seq_str = " ".join(chunk)
            separator = " |" if i < max_chunk else ""
            print(f"    ({i}, {i + 1}): [{seq_str}]{separator}")
            
        print("  }")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Convert a MIDI file to MMN format.")
    
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
        choices=["raw", "seqploop"], 
        default="seqploop", 
        help="Output format: 'raw' for a flat list, or 'seqploop' for chunked cycles. Default: 'seqploop'"
    )
    parser.add_argument(
        "-n", "--notes", 
        type=int, 
        default=8, 
        help="Notes per cycle when using 'seqploop' format. Default: 8"
    )

    args = parser.parse_args()

    convert_midi_to_intervals(
        filepath=args.filepath, 
        target_scale=args.scale, 
        output_format=args.format, 
        notes_per_cycle=args.notes
    )
