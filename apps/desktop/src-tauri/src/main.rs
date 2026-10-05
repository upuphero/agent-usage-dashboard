#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|value| value == "--diagnose-usage")
    {
        if arguments.len() != 2
            || usage_desktop::diagnose_usage(std::path::Path::new(&arguments[1])).is_err()
        {
            std::process::exit(1);
        }
        return;
    }
    usage_desktop::run();
}
