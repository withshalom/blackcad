// BlackCAD for Windows, macOS and Linux (lib.rs). On Windows the release
// build is a windowed program, with no console beside it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    blackcad_app_lib::run()
}
