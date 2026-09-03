use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct AppConfig {
    pub mmn_directory: String,
    pub midi_port: Option<String>,
}

fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(mut home) = dirs::home_dir() {
            home.push(&path[2..]);
            return home;
        }
    }
    PathBuf::from(path)
}

pub fn initialize_config() -> (AppConfig, PathBuf, PathBuf) {
    let config_dir = dirs::config_dir()
        .expect("Could not find user config directory")
        .join("cycle_midi");

    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create cycle_midi config directory");
    }

    let config_path = config_dir.join("config.toml");

    if !config_path.exists() {
        let default_workspace = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("cycle_midi_workspace");

        let default_config_content = format!(
            "# cycle_midi configuration\n# Specify the absolute path or use ~/ for your home directory\nmmn_directory = \"{}\"\n# Optional: Specify a default MIDI output port name to connect to\n# midi_port = \"Midi Through Port-0\"\n",
            default_workspace.display()
        );
        fs::write(&config_path, default_config_content)
            .expect("Failed to write default config.toml");
        println!(
            "Created default configuration file at: {}",
            config_path.display()
        );
    }

    let config_str = fs::read_to_string(&config_path).expect("Failed to read config.toml");
    let config: AppConfig = toml::from_str(&config_str).expect("Failed to parse config.toml");

    let mmn_dir = expand_tilde(&config.mmn_directory);
    if !mmn_dir.exists() {
        fs::create_dir_all(&mmn_dir).expect("Failed to create designated MMN directory");
        println!("Created MMN workspace directory at: {}", mmn_dir.display());
    }

    let file_path = mmn_dir.join("live.mmn");

    if !file_path.exists() {
        fs::write(
            &file_path,
            "#BPM=120\n#SCALE=C4 minor\nT1: 0 2 3 4 . 7 _\nT2(G3 minor_pentatonic): {-7 | 0}",
        )
        .expect("Failed to create initial file");
    }

    (config, mmn_dir, file_path)
}
