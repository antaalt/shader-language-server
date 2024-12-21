# Shader sense

[![shader-sense](https://img.shields.io/crates/v/shader-sense)](https://crates.io/crates/shader-sense)

Shader sense is a library trying to unify shader exploration at runtime with validation and symbol request, intended as use in a language server. This works through the use of standard API. It can be built to desktop or [WASI](https://wasi.dev/). WASI will let the extension run even in browser, but it suffer from limitations. See below for more informations.

- **GLSL** uses [glslang-rs](https://github.com/SnowflakePowered/glslang-rs) as backend. It provide complete linting for GLSL trough glslang API bindings from C.
- **HLSL** uses [hassle-rs](https://github.com/Traverse-Research/hassle-rs) as backend. It provides bindings to directx shader compiler in rust.
- **WGSL** uses [naga](https://github.com/gfx-rs/naga) as backend for linting.

For symbol, the API is relying on abstract syntax tree. As we want to support different language, and to ease this process, we are using the [tree-sitter](https://tree-sitter.github.io/tree-sitter/) API (instead of standard API), which generate AST with query support, and is already available in a lot of languages.

## Binaries

### Shader language server

This library is used in a language server as [shader-language-server](https://github.com/antaalt/shader-sense/tree/main/shader-language-server). 

### Shader intrinsic parser

This library is used to parse intrinsic language documentations with [shader-intrinsic-parser](https://github.com/antaalt/shader-sense/tree/main/shader-intrinsic-parser).

### Example

You can find example of the library [here](https://github.com/antaalt/shader-sense/tree/main/shader-sense/examples).

## Build for WASI

The library can be built using [WASI](https://wasi.dev/) for web support. We are using threads so we target the thread version.

To build it, install target first :

```shell
rustup target add wasm32-wasip1-threads
```

Then build the app with:

```shell
cargo build --target wasm32-wasip1-threads
```

### Dependencies

You will need to install clang. You will also need to setup the environment variable `WASI_SYSROOT` as well targetting the wasi sysroot folder which you can find at [WASI SDK repo](https://github.com/WebAssembly/wasi-sdk) in releases so that cc-rs can build c++ correctly.

### DirectX Shader Compiler issue

Right now, the server can lint hlsl sm 6 through [hassle-rs](https://github.com/Traverse-Research/hassle-rs). It relies on [DirectX Shader Compiler](https://github.com/microsoft/DirectXShaderCompiler) which cannot be built statically. Or, WASI cannot handle dll as of now, and so we need to compile it statically to link it. There is an [ongoing issue](https://github.com/Traverse-Research/hassle-rs/issues/57) for that at hassle rs, but it seems to be complicated, as explained [here](https://devlog.hexops.com/2024/building-the-directx-shader-compiler-better-than-microsoft/). So with WASI, this extension relies instead on glslang to lint hlsl. It only support basic features of shader models 6.0 and some of upper versions, but many recent added features will be missing from linter. As of now, there is not much way to fix this easily, except hoping that Microsoft does something about this.