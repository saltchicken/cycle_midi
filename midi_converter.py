import mido

def midi_to_note_name(midi_note):
    notes = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
    # mido note 60 is Middle C (C4)
    octave = (midi_note // 12) - 1
    note = notes[midi_note % 12]
    return f"{note}{octave}"

def convert_midi_to_flat_sequence(filepath):
    mid = mido.MidiFile(filepath)
    
    notes = []
    
    # Check all tracks for Note On events
    for track in mid.tracks:
        for msg in track:
            if msg.type == 'note_on' and msg.velocity > 0:
                notes.append(midi_to_note_name(msg.note))

    # Output as a flat mmn sequence
    print("T1:")
    print("  " + " ".join(notes))

if __name__ == "__main__":
    convert_midi_to_flat_sequence("PreludeCMajor.mid")
