mod common;
#[cfg(not(target_os = "wasi"))]
mod dxc;
mod glslang;
mod include;
mod naga;
mod server;
mod shader_error;

pub fn main() {
    env_logger::init();
    server::run();
}
