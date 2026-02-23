mod config;
mod icon_extractor;
mod window;

use windows::core::Error;

fn main() -> Result<(), Error> {
    println!("Loading config...");
    match config::load_config("config.json") {
        Ok(cfg) => {
            println!(
                "Config loaded successfully with {} categories.",
                cfg.categories.len()
            );
            println!("Starting window message loop...");
            if let Err(e) = window::run(cfg) {
                eprintln!("Fatal error in window::run: {:?}", e);
            }
        }
        Err(e) => eprintln!("Failed to load config: {}", e),
    }

    Ok(())
}
