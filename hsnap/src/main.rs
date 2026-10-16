#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod dialogs;
mod icons;
mod capture;
mod compose;
mod editor;
mod hotkeys;
mod picker;
mod print;
mod print_gdi;
mod print_ui;
mod quicksave;
mod scroll;
mod service;
mod stamps;
mod stamps_ui;
mod startup;
mod stitch;
mod tray;

fn main() {
    capture::make_dpi_aware();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Release builds have no console of their own; let the command-line modes print to the parent one.
    if args.first().is_some_and(|a| a.starts_with("--cli")) {
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(windows::Win32::System::Console::ATTACH_PARENT_PROCESS);
        }
    }
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
        // Headless auto-scroll: hsnap --cli-scroll <x> <y> <out.png>
        Some("--cli-scroll") if args.len() >= 4 => {
            let (x, y) = (args[1].parse().unwrap_or(0), args[2].parse().unwrap_or(0));
            match scroll::capture_scrolling((x, y), 300, 30_000).and_then(|img| img.save(&args[3]).map(|_| img.height()).map_err(|e| e.to_string())) {
                Ok(h) => println!("saved {} ({h} px tall)", args[3]),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        // Headless print test: hsnap --cli-print <in.png> <printer name> <out file>
        Some("--cli-print") if args.len() >= 4 => {
            let img = match image::open(&args[1]) {
                Ok(i) => i.to_rgba8(),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            };
            let setup = print::PageSetup { header: "HSnap test - page %page% of %pages%".into(), footer: "%file%".into(), footer_justify: print::Justify::Right, draw_frame: true, ..Default::default() };
            let target = print_gdi::Target::Named { printer: args[2].clone(), output: Some(args[3].clone().into()) };
            match print_gdi::print_image(&img, &setup, "test.png", target) {
                Ok(()) => println!("printed to {}", args[3]),
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
