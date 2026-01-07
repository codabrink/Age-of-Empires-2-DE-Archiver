#![windows_subsystem = "windows"]

use aoe_archive::launch;

fn main() {
    if let Err(err) = launch() {
        println!("App crashed: {err:?}");
    }
}
