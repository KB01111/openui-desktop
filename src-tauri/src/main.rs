// Keeps the release build free of a console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    helios_ai_lib::run()
}
