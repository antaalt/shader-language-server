use log::info;

mod server;
mod shaders;

fn get_version() -> &'static str {
    static VERSION: &str = env!("CARGO_PKG_VERSION");
    return VERSION;
}

fn print_version() {
    println!("shader_language_server v{}", get_version());
}

fn run_server() {
    env_logger::init();
    info!("shader_language_server v{}", get_version());
    server::run();
}

pub fn main() {
    let last = std::env::args().last();
    match last {
        Some(last) => match last.as_str() {
            "--version" => print_version(),
            "-v" => print_version(),
            _ => run_server(),
        },
        None => run_server(),
    }
}
