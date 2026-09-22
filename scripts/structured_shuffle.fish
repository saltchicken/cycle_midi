#!/usr/bin/env fish

set scene_dir /home/saltchicken/Projects/cycle_midi/examples
set setlist \
    $scene_dir/ambient_generative.mmn \
    $scene_dir/melodic_techno.mmn \
    $scene_dir/live3.mmn

while true
    for scene in $setlist
        echo "Staging "(basename $scene)"..."
        touch $scene
        sleep 120 # Play each scene for 2 minutes
    end
end
