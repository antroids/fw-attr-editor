// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::application::{Application, Status};
use clap::Parser;
use std::path::Path;

mod sysfs_firmware_attributes;

mod application;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to Firmware Attributes SysFs directory, for example "/sys/class/firmware-attributes/thinklmi/"
    #[arg(short, long)]
    path: Option<String>,

    /// Log level, possible values are: trace, debug, info, warn, error.
    /// Can be specified with LOG_STYLE env variable. Default: warn;
    #[arg(short, long)]
    log_level: Option<String>,
}

fn main() -> Result<(), eframe::Error> {
    let args = Args::parse();
    let env = env_logger::Env::default()
        .filter_or("LOG_LEVEL", args.log_level.unwrap_or("warn".to_string()))
        .write_style_or("LOG_STYLE", "always");

    env_logger::init_from_env(env);

    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(640.0, 480.0)),
        ..Default::default()
    };
    let application = if let Some(root) = args.path {
        Application::bios_admin_authentication(Path::new(&root), &Status::default())
            .unwrap_or(Application::select_root(Vec::new()))
    } else {
        Application::autodetect_root()
    };
    eframe::run_native(
        "BIOS Settings Editor",
        options,
        Box::new(|_cc| Box::new(application)),
    )
}
