mod server;
mod shaders;
pub fn main() {
    env_logger::init();
    server::run();
}
