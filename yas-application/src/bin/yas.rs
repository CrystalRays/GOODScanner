use clap::{command, Command};
use yas::utils::press_any_key_to_continue;
use yas_genshin::application::{CombinedScannerApplication, GoodScannerApplication};

fn get_genshin_command() -> Command {
    let cmd = CombinedScannerApplication::build_command();
    cmd.name("genshin")
}

fn get_good_command() -> Command {
    let cmd = GoodScannerApplication::build_command();
    cmd.name("good")
}

fn init() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();
}

pub fn main() {
    init();
    let cmd = command!()
        .subcommand(get_genshin_command())
        .subcommand(get_good_command());
    let arg_matches = cmd.get_matches();

    let res = if let Some((subcommand_name, matches)) = arg_matches.subcommand() {
        match subcommand_name {
            "genshin" => {
                let application = CombinedScannerApplication::new(matches.clone());
                application.run()
            }
            "good" => {
                let application = GoodScannerApplication::new(matches.clone());
                application.run()
            }
            _ => {
                // Default: run good scanner when no subcommand is given
                println!("[yas] No subcommand specified, defaulting to 'good' scanner.");
                let application = GoodScannerApplication::new(arg_matches.clone());
                application.run()
            }
        }
    } else {
        // No subcommand at all (e.g. double-clicked the exe) — run good scanner
        println!("[yas] No subcommand specified, defaulting to 'good' scanner.");
        let application = GoodScannerApplication::new(arg_matches.clone());
        application.run()
    };

    match res {
        Ok(_) => {
            press_any_key_to_continue();
        },
        Err(e) => {
            log::error!("error: {}", e);
            press_any_key_to_continue();
        }
    }
}
