import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { request, responseResult } from "../resources/ui/protocol.js";

test("browser RPC correlates responses and preserves result data", () => {
  assert.deepEqual(request("ping", undefined, 7), { jsonrpc: "2.0", method: "ping", id: 7 });
  assert.equal(responseResult({ jsonrpc: "2.0", id: 7, result: null }, 7), null);
});

test("browser RPC rejects malformed and mismatched responses", () => {
  for (const response of [
    { id: 7, result: "pong" },
    { jsonrpc: "1.0", id: 7, result: "pong" },
    { jsonrpc: "2.0", id: 8, result: "pong" },
    { jsonrpc: "2.0", id: 7 },
    { jsonrpc: "2.0", id: 7, result: "pong", error: {} },
    { jsonrpc: "2.0", id: 7, error: { code: "bad", message: 42 } },
  ]) assert.throws(() => responseResult(response, 7));
  assert.throws(() => responseResult({ jsonrpc: "2.0", id: 7, error: { code: -32603, message: "failed" } }, 7), /failed/);
});

test("browser entrypoint loads the shared protocol as a module", () => {
  const html = readFileSync(new URL("../resources/ui/index.html", import.meta.url), "utf8");
  assert.match(html, /<script type="module" src="\/app.js"><\/script>/);
});
