// Release builds get no console window on Windows; debug builds keep one,
// because that is where a panic before the window opens is readable.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = mnema_desktop::run() {
        // `{error:#}` prints the whole context chain. A bare `{error}` would
        // print "running the application" and drop the cause.
        eprintln!("fatal: {error:#}");
        std::process::exit(1);
    }
}
