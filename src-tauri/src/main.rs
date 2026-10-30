// Release: GUI subsystem, so no console window opens behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    netlume_lib::run();
}
