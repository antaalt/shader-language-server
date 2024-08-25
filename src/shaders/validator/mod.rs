#[cfg(not(target_os = "wasi"))]
pub mod dxc;
pub mod glslang;
pub mod naga;
pub mod validator;
