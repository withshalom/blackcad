// The core's own thread on the web: core.wasm runs here, so reading a big
// drawing never freezes the page. core.js (WorkerBackend) sends one call at
// a time as {seq, method, args}; each answer carries the same seq, the
// value, and the bytes the core holds. Bytes travel as transfers, not copies.

import { WasmEngine, errorData, runEngineCall } from "./core.js";

let engine = null;

self.onmessage = async ({ data }) => {
  const { seq, method, args } = data;
  try {
    if (method === "start") {
      engine = await WasmEngine.load(args[0]);
      return self.postMessage({ seq, value: engine.version });
    }
    const value = runEngineCall(engine, method, args);
    const live = engine.liveBytes();
    const transfer = [];
    if (value instanceof Uint8Array) transfer.push(value.buffer);
    else if (value?.bytes instanceof Uint8Array) transfer.push(value.bytes.buffer);
    self.postMessage({ seq, value, live }, transfer);
  } catch (e) {
    self.postMessage({ seq, error: errorData(e) });
  }
};
