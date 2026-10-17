#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    let mut args = std::env::args();
    let _ = args.next();
    if args.next().as_deref() == Some("mcp") {
        localboard_lib::mcp_stdio();
        return;
    }
    localboard_lib::run();
}
