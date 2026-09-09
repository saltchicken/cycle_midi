#!/usr/bin/env fish

set scene_dir /home/saltchicken/Projects/cycle_midi/examples

while true
    # Pick a random .mmn file from the directory
    set random_scene (random choice $scene_dir/*.mmn)

    echo "Staging "(basename $random_scene)" for the next phrase boundary..."

    # Trigger the Rust file watcher
    touch $random_scene

    # Wait roughly 60 seconds before picking the next one
    sleep 240
end
