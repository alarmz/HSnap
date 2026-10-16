mod app;
mod dialogs;
mod icons;
mod capture;
mod compose;
mod editor;
mod hotkeys;
mod picker;
mod quicksave;
mod service;

fn main() {
    capture::make_dpi_aware();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        // Internal: selection overlay used by the GUI.
        Some("--pick") => match picker::Params::from_args(&args[1..]) {
            Some(p) => std::process::exit(picker::run(p)),
            None => std::process::exit(2),
        },
        // Headless capture: hsnap --cli <full|virtual|active|cursor> <out.png>
        Some("--cli") if args.len() >= 3 => {
            let res = match args[1].as_str() {
                "full" => capture::capture_full_screen(false),
                "virtual" => capture::capture_virtual_desktop(false),
                "active" => capture::capture_active_window(false),
                "cursor" => capture::capture_cursor_only([255, 255, 255, 255]),
                other => Err(format!("unknown mode {other}")),
            };
            match res.and_then(|img| img.save(&args[2]).map_err(|e| e.to_string())) {
                Ok(()) => println!("saved {}", args[2]),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some("--help") | Some("-h") => {
            println!("hsnap            start the GUI\nhsnap --cli <full|virtual|active|cursor> <out.png>");
        }
        _ => {
            let opts = eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default().with_title(service::WINDOW_TITLE).with_inner_size([1000.0, 620.0]),
                ..Default::default()
            };
            let _ = eframe::run_native(service::WINDOW_TITLE, opts, Box::new(|cc| Ok(Box::new(app::App::new(cc)))));
        }
    }
}
