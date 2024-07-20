mod common;
#[cfg(not(target_os = "wasi"))]
mod dxc;
mod glslang;
mod naga;
mod server;
mod shader_error;

pub fn main() {
    server::run();
}
