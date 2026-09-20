import mido
import re

# The exact interval definitions from your src/parser/directives.rs
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
    """Converts a pitch literal like C4 or D#3 to a raw MIDI note number."""
    notes = {"C": 0, "C#": 1, "DB": 1, "D": 2, "D#": 3, "EB": 3, 
             "E": 4, "F": 5, "F#": 6, "GB": 6, "G": 7, "G#": 8, 
             "AB": 8, "A": 9, "A#": 10, "BB": 10, "B": 11}
    
    match = re.match(r"([A-Ga-g][#bB]?)(-?\d+)", note_name)
    if not match:
        return 60  # Default to C4 if unparseable
        
    note_str = match.group(1).upper()
    octave = int(match.group(2))
    
    base = notes.get(note_str, 0)
    return (octave + 1) * 12 + base

def midi_to_scale_degree(midi_note, root_midi, scale_intervals):
    """Reverses the logic in your resolve_pitch function."""
    diff = midi_note - root_midi
    
    # Python's // and % operate identically to Rust's div_euclid / rem_euclid
    octave_diff = diff // 12
    pc_diff = diff % 12
    scale_len = len(scale_intervals)
    
    # 1. Exact match in the scale
    if pc_diff in scale_intervals:
        degree_index = scale_intervals.index(pc_diff)
        numeric_degree = (octave_diff * scale_len) + degree_index
        return str(numeric_degree)
        
    # 2. Out of scale: Find the closest interval below it and add a sharp
    closest_idx = 0
    for i, interval in enumerate(scale_intervals):
        if interval < pc_diff:
            closest_idx = i
            
    numeric_degree = (octave_diff * scale_len) + closest_idx
    accidental = pc_diff - scale_intervals[closest_idx]
    
    return f"{numeric_degree}{'#' * accidental}"

def convert_midi_to_intervals(filepath, target_scale="C4 major"):
    mid = mido.MidiFile(filepath)
    
    # Setup the scale definitions
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

    # Output your MMN format
    print(f"#SCALE={target_scale}")
    print("T1:")
    print("  " + " ".join(notes))

if __name__ == "__main__":
    # Just update this to the scale you want to lock the intervals to
    convert_midi_to_intervals("PreludeCMajor.mid", "C4 major")
