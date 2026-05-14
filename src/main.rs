#![allow(unused, clippy::all)]

mod bit_array;
mod simulation;
mod sim_builder;
mod editor;

use editor::App;

fn main() -> Result<(), eframe::Error> {
    App::run()
}
