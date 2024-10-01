# Shader language server

This application is a language server for shaders (HLSL, GLSL, WGSL) that is mainly meant to be used as a server for vscode extension [shader-validator](https://github.com/antaalt/shader-validator). It is following the [LSP protocol](https://microsoft.github.io/language-server-protocol/) to communicate with the extension so it could be used with any editor supporting it. It can be built to desktop or [WASI](https://wasi.dev/). WASI will let the extension run even in web version of vscode, but it suffer from limitations. See below for more informations.

## Features

This language server only support a few options :

- **Diagnostics**: lint the code as you type.
- **Completion**: suggest completion values as you type.
- **Signature**: view the signatures of the current function.
- **Hover**: view the declaration of an element by hovering it.
- **Goto**: allow to go to declaration of an element.

The server support HLSL, GLSL, WGSL diagnostics, but as of now, only GLSL provides some symbols for completion, signatures, hover & goto.

### Diagnostics

Diagnostics are generated following language specifics API:

- **GLSL** uses [glslang-rs](https://github.com/SnowflakePowered/glslang-rs) as backend. It provide complete linting for GLSL trough glslang API bindings from C.
- **HLSL** uses [hassle-rs](https://github.com/Traverse-Research/hassle-rs) as backend. It provides bindings to directx shader compiler in rust.
- **WGSL** uses [naga](https://github.com/gfx-rs/naga) as backend for linting.

## Build for WASI

The server can be built using [WASI](https://wasi.dev/) to interface with [VS Code WASI](https://code.visualstudio.com/blogs/2023/06/05/vscode-wasm-wasi) support. We are using threads so we target the thread version.

To build it, install target first :
```shell
rustup target add wasm32-wasip1-threads
```

Then build the app with:

```shell
cargo build --target wasm32-wasip1-threads
```

### Dependencies

You will need to install clang. You will need to setup the environment variable `WASI_SYSROOT` as well targetting the wasi sysroot folder which you can find at [WASI SDK repo](https://github.com/WebAssembly/wasi-sdk) in releases so that cc-rs can build c++ correctly.

### DirectX Shader Compiler issue

Right now, the server can lint hlsl sm 6.0 through [hassle-rs](https://github.com/Traverse-Research/hassle-rs). It relies on [DirectX Shader Compiler](https://github.com/microsoft/DirectXShaderCompiler) which cannot be built statically. Or, WASI cannot handle dll as of now, and so we need to compile it statically to link it. There is an [ongoing issue](https://github.com/Traverse-Research/hassle-rs/issues/57) for that at hassle rs, but it seems to be complicated, as explained [here](https://devlog.hexops.com/2024/building-the-directx-shader-compiler-better-than-microsoft/). So with WASI, this extension relies instead on glslang to lint hlsl. It only support basic features of shader models 6.0 and some of upper versions, but many recent added features will be missing from linter. As of now, there is not much way to fix this easily, except hoping that Microsoft does something about this.