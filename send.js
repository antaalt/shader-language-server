
function send(message) {
    const ContentLength = 'Content-Length: ';
    const CRLF = '\r\n';

	process.stderr.write('\n[shader_language_server::out]' + message +'\n');
    const buffer = Buffer.from(message, 'utf8');
    const headers = [];
    headers.push(ContentLength + buffer.length.toString(), CRLF, CRLF);
    process.stdout.write(headers.join(''), 'ascii');
    process.stdout.write(buffer);
}
function request(method, id, params) {
    send('{"jsonrpc": "2.0", "method": "' + method + '", "id":' + id + ', "params": ' + JSON.stringify(params) + '}');
}
function notification(method, params) {
    send('{"jsonrpc": "2.0", "method": "' + method + '", "params": ' + JSON.stringify(params) + '}');
}
function sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}

process.stdin.on('data', (data) => {
	const content = data.toString();
	process.stderr.write('[shader_language_server::in]' + content);
});
// Ensure WASM is running smoothly
// From: https://github.com/microsoft/vscode-wasm/blob/main/testbeds/lsp-rust/server/send.js
// Seems to be hanging on vscode: https://github.com/microsoft/vscode-wasm/issues/23
setTimeout(async () => {
    request("initialize", 1, {"capabilities": {}});
    await sleep(500);
    notification("initialized", {});
    await sleep(500);
    request("textDocument/didOpen", 2, {"textDocument": {"uri": "file://test/glsl/ok.frag.glsl", "language_id":"glsl"}});
    await sleep(500);
    request("textDocument/definition", 3, {"textDocument": {"uri": "file://test/glsl/ok.frag.glsl"}, "position": {"line": 1, "character": 1}});
    await sleep(500);
    request("textDocument/didClose", 2, {"textDocument": {"uri": "file://test/glsl/ok.frag.glsl", "language_id":"glsl"}});
    await sleep(500);
    request("shutdown", 4, {});
    await sleep(500);
    notification("exit", {});
}, 1000);