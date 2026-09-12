// JSON-RPC envelope boundary for the browser client.
export function request(method, params, id) {
  const document = { jsonrpc: "2.0", method, id };
  if (params !== undefined) document.params = params;
  return document;
}

export function responseResult(response, expectedId) {
  if (!response || typeof response !== "object" || Array.isArray(response) ||
      response.jsonrpc !== "2.0" || response.id !== expectedId) {
    throw new Error("Invalid JSON-RPC response envelope or ID");
  }
  const hasResult = Object.hasOwn(response, "result");
  const hasError = Object.hasOwn(response, "error");
  if (hasResult === hasError) throw new Error("Expected exactly one of result or error");
  if (hasError) {
    if (!response.error || !Number.isInteger(response.error.code) ||
        typeof response.error.message !== "string") {
      throw new Error("Invalid JSON-RPC error");
    }
    throw new Error(`${response.error.code}: ${response.error.message}`);
  }
  return response.result;
}
