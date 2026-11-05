// The CAD core as the workbench sees it.
//
// `Core` is the one interface app.js uses. Every call is asynchronous and
// calls are answered in the order they were made, whichever engine is
// behind it:
//
//   - the native engine, linked into the BlackCAD app (apps/shell), when the
//     page runs inside it;
//   - core.wasm (crates/cad-ffi built for wasm32-unknown-unknown) in a Worker
//     (core-worker.js), on the web;
//   - core.wasm on the page's own thread, where no Worker can be made (the
//     single-file test page, Node).
//
// `WasmEngine` is the synchronous glue to core.wasm's entry points
// (crates/cad-ffi/src/web.rs). The module imports nothing; this file is all
// the glue there is, and it imports nothing either.

const enc = new TextEncoder();
const dec = new TextDecoder();

/** Thrown when the core stops on a bug (a Rust panic). The module is replaced. */
export class CoreCrash extends Error {}

/** core.wasm, called directly and synchronously. Runs in the Worker, or on
 *  the page's thread where there is none. */
export class WasmEngine {
  static async load(url = "core.wasm") {
    let bytes;
    const inline = globalThis.CAD_CORE_WASM; // a single-file page carries the module inline, as base64
    if (typeof inline === "string") {
      const s = atob(inline);
      bytes = new Uint8Array(s.length);
      for (let i = 0; i < s.length; i++) bytes[i] = s.charCodeAt(i);
    } else {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`core.wasm: ${res.status} ${res.statusText}`);
      bytes = await res.arrayBuffer();
    }
    const module = await WebAssembly.compile(bytes);
    const core = new WasmEngine(module);
    await core.start();
    return core;
  }

  constructor(module) {
    this.module = module;
  }

  async start() {
    const inst = await WebAssembly.instantiate(this.module, {});
    this.x = inst.exports;
    if (this.x.cad_abi_version() !== 1) throw new Error("core.wasm is from a different build");
    this.x.cad_version();
    this.version = this.text();
  }

  /** The result buffer, copied out. Addresses are unsigned: past 2 GiB a
   *  pointer read as a signed number would be negative. */
  bytes() {
    return this.guard(() => {
      const x = this.x;
      return new Uint8Array(x.memory.buffer, x.cad_result_ptr() >>> 0, x.cad_result_len() >>> 0).slice();
    });
  }
  text() {
    return dec.decode(this.bytes());
  }

  /** Run `fn` with byte arrays copied into the module's memory. */
  withBytes(arrays, fn) {
    const x = this.x;
    const ptrs = this.guard(() =>
      arrays.map((a) => {
        if (!a.length) return 0;
        const p = x.cad_alloc(a.length) >>> 0;
        // Null: the core could not grow its memory. Writing there would
        // overwrite the core's own data.
        if (!p) throw new RangeError(`the core could not allocate ${a.length} bytes`);
        new Uint8Array(x.memory.buffer, p, a.length).set(a);
        return p;
      })
    );
    try {
      return this.guard(() => fn(ptrs));
    } finally {
      if (!this.crashed) arrays.forEach((a, i) => a.length && x.cad_free(ptrs[i], a.length));
    }
  }

  /** Calls into the core. A trap (a panic, or memory the core could not
   *  get), a RangeError (memory that could not grow, an address outside
   *  memory, a stack overflow) or a failed allocation all mean the module
   *  can no longer be trusted: it is marked crashed, and the app restarts
   *  it and reopens its drawings. */
  guard(fn) {
    if (this.crashed) throw new CoreCrash("the core stopped earlier and has not been restarted");
    try {
      return fn();
    } catch (e) {
      if (e instanceof CoreCrash) throw e;
      if (e instanceof WebAssembly.RuntimeError || e instanceof RangeError) {
        let msg = "";
        if (e instanceof WebAssembly.RuntimeError) {
          try {
            this.x.cad_panic_message();
            const x = this.x;
            msg = dec.decode(new Uint8Array(x.memory.buffer, x.cad_result_ptr() >>> 0, x.cad_result_len() >>> 0));
          } catch (_) {}
        }
        this.crashed = true;
        const mb = Math.round(this.memoryBytes() / 2 ** 20);
        const oom = !msg && (e instanceof RangeError || mb >= SOFT_LIMIT_MB);
        const c = new CoreCrash(oom ? `out of memory (${mb} MB in use): ${e.message}` : msg || String(e));
        c.outOfMemory = oom;
        throw c;
      }
      throw e;
    }
  }

  /** Size of the core's memory, in bytes (it only grows). */
  memoryBytes() {
    try {
      return this.x.memory.buffer.byteLength;
    } catch (_) {
      return 0;
    }
  }

  /** Bytes the core holds right now (its allocations, not its high-water mark). */
  liveBytes() {
    return this.x.cad_heap_bytes ? this.x.cad_heap_bytes() >>> 0 : this.memoryBytes();
  }

  /** A fresh module after a crash. Every open drawing is gone. */
  async restart() {
    this.crashed = false;
    await this.start();
  }

  open(bytes) {
    const id = this.withBytes([bytes], ([p]) => this.x.cad_open(p, bytes.length));
    if (!id) throw new Error(this.text());
    return id;
  }
  newDrawing(imperial) {
    const id = this.guard(() => this.x.cad_new(imperial ? 1 : 0));
    if (!id) throw new Error(this.text());
    return id;
  }
  /** The sample plate from the workspace mockups. */
  sample() {
    const id = this.guard(() => this.x.cad_sample());
    if (!id) throw new Error(this.text());
    return id;
  }
  close(id) {
    this.guard(() => this.x.cad_close(id));
  }
  info(id) {
    const ok = this.guard(() => this.x.cad_info(id));
    if (!ok) throw new Error(this.text());
    return JSON.parse(this.text());
  }
  /** The display list, as bytes (crates/cad-core/src/export/display.rs). */
  displayBytes(id) {
    const ok = this.guard(() => this.x.cad_display(id));
    if (!ok) throw new Error(this.text());
    return this.bytes();
  }
  /** What changed since the display was last read, as bytes. */
  displayChangesBytes(id) {
    const ok = this.guard(() => this.x.cad_display_changes(id));
    if (!ok) throw new Error(this.text());
    return this.bytes();
  }
  display(id) {
    return parseDisplay(this.displayBytes(id));
  }
  /** What changed since the display was last read: `patch` lists the
   *  entities whose items it replaces, or it is the whole list. */
  displayChanges(id) {
    return parseDisplay(this.displayChangesBytes(id));
  }
  /** Run a command line (crates/cad-ffi/src/session.rs); its JSON answer. */
  exec(id, line) {
    const b = enc.encode(line);
    const ok = this.withBytes([b], ([p]) => this.x.cad_exec(id, p, b.length));
    const t = this.text();
    if (!ok) throw new Error(t);
    return JSON.parse(t);
  }
  /** Try a command; null instead of an error. */
  query(id, line) {
    try {
      return this.exec(id, line);
    } catch (e) {
      if (e instanceof CoreCrash) throw e;
      return null;
    }
  }
  save(id, format, user = "") {
    const f = enc.encode(format);
    const u = enc.encode(user);
    const ok = this.withBytes([f, u], ([fp, up]) =>
      this.x.cad_save(id, fp, f.length, Date.now() / 1000, up, u.length)
    );
    if (!ok) throw new Error(this.text());
    const bytes = this.bytes();
    this.guard(() => this.x.cad_save_notes());
    const notes = this.text();
    return { bytes, notes: notes ? notes.split("\n") : [] };
  }
}

/** The core refuses new drawings and edits above this in WebAssembly
 *  (crates/cad-ffi/src/web.rs), well below the 2 GiB where browsers start
 *  to fail. The native engine has no such ceiling. */
const SOFT_LIMIT_MB = 1024;

/** A failed call's error, rebuilt on this side of a Worker or the app's bridge. */
function rebuildError(e) {
  if (e && e.crash) {
    const c = new CoreCrash(e.message);
    c.outOfMemory = !!e.outOfMemory;
    return c;
  }
  return new Error(e?.message ?? String(e));
}

/** The error as data that can cross to the page. */
export function errorData(e) {
  return { message: e?.message ?? String(e), crash: e instanceof CoreCrash, outOfMemory: !!e?.outOfMemory };
}

/** Runs a WasmEngine call: the methods a backend may be asked for, and how
 *  their answers travel (bytes as bytes, the rest as data). */
export function runEngineCall(engine, method, args) {
  switch (method) {
    case "open": return engine.open(args[0]);
    case "newDrawing": return engine.newDrawing(args[0]);
    case "sample": return engine.sample();
    case "close": return engine.close(args[0]);
    case "info": return engine.info(args[0]);
    case "display": return engine.displayBytes(args[0]);
    case "displayChanges": return engine.displayChangesBytes(args[0]);
    case "exec": return engine.exec(args[0], args[1]);
    case "save": return engine.save(args[0], args[1], args[2]);
    default: throw new Error(`the core has no call "${method}"`);
  }
}

/** core.wasm on this thread, behind the same asynchronous face as the others. */
class InThreadBackend {
  kind = "wasm";
  async start(url) {
    this.engine = await WasmEngine.load(url);
    return { version: this.engine.version };
  }
  async call(method, args) {
    const value = runEngineCall(this.engine, method, args);
    return { value, live: this.engine.liveBytes() };
  }
  async restart() {
    await this.engine.restart();
  }
  get crashed() {
    return !!this.engine?.crashed;
  }
}

/** core.wasm in a Worker (core-worker.js): the page's thread stays free
 *  while the core reads a big drawing. */
class WorkerBackend {
  kind = "wasm";
  seq = 0;
  pending = new Map();
  crashed = false;
  constructor(url) {
    this.url = url;
  }
  spawn() {
    this.worker = new Worker(new URL("core-worker.js", import.meta.url), { type: "module" });
    this.worker.onmessage = ({ data }) => {
      const p = this.pending.get(data.seq);
      if (!p) return;
      this.pending.delete(data.seq);
      if (data.error) {
        if (data.error.crash) this.crashed = true;
        p.reject(rebuildError(data.error));
      } else p.resolve(data);
    };
    // The Worker itself died (its memory, a script error): every call in
    // flight is lost, as in a crash.
    this.worker.onerror = (e) => {
      e.preventDefault?.();
      this.crashed = true;
      const c = new CoreCrash(e.message || "the core's worker stopped");
      for (const p of this.pending.values()) p.reject(c);
      this.pending.clear();
    };
  }
  post(method, args = [], transfer = []) {
    if (this.crashed && method !== "start") return Promise.reject(new CoreCrash("the core stopped earlier and has not been restarted"));
    const seq = ++this.seq;
    return new Promise((resolve, reject) => {
      this.pending.set(seq, { resolve, reject });
      this.worker.postMessage({ seq, method, args }, transfer);
    });
  }
  async start() {
    this.spawn();
    const r = await this.post("start", [this.url]);
    return { version: r.value };
  }
  call(method, args) {
    return this.post(method, args);
  }
  async restart() {
    this.worker.terminate();
    for (const p of this.pending.values()) p.reject(new CoreCrash("the core was restarted"));
    this.pending.clear();
    this.crashed = false;
    await this.start();
  }
}

/** The engine inside the BlackCAD app (apps/shell), when the page runs
 *  there: native code with the computer's memory and threads. The app
 *  serves a small bridge object; see apps/shell. */
class NativeBackend {
  kind = "native";
  crashed = false;
  constructor(bridge) {
    this.bridge = bridge;
  }
  async start() {
    return { version: await this.bridge.version() };
  }
  async call(method, args) {
    try {
      return await this.bridge.call(method, args);
    } catch (e) {
      const err = rebuildError(e);
      if (err instanceof CoreCrash) this.crashed = true;
      throw err;
    }
  }
  async restart() {
    await this.bridge.restart();
    this.crashed = false;
  }
}

/**
 * The core, as the workbench uses it. Every method returns a promise, and
 * calls run one at a time in the order they were made: an edit made after
 * another always sees it, and undo and redo stay in step.
 */
export class Core {
  static SOFT_LIMIT_MB = SOFT_LIMIT_MB;

  /** Starts the best engine there is: the app's native one, else
   *  core.wasm in a Worker, else core.wasm on this thread. */
  static async load(url = "core.wasm", opts = {}) {
    const bridge = opts.bridge ?? globalThis.BLACKCAD_NATIVE;
    const inline = typeof globalThis.CAD_CORE_WASM === "string";
    let backend;
    if (bridge && opts.native !== false) backend = new NativeBackend(bridge);
    else if (!inline && opts.worker !== false && typeof Worker !== "undefined") {
      const abs = typeof location !== "undefined" ? new URL(url, location.href).href : url;
      backend = new WorkerBackend(abs);
    } else backend = new InThreadBackend();
    let started;
    try {
      started = await backend.start(url);
    } catch (e) {
      // A Worker that will not start (an old engine without module
      // workers, a page opened from disk): the same core on this thread.
      if (!(backend instanceof WorkerBackend)) throw e;
      backend.worker?.terminate();
      backend = new InThreadBackend();
      started = await backend.start(url);
    }
    const core = new Core(backend);
    core.version = started.version;
    return core;
  }

  constructor(backend) {
    this.backend = backend;
    /** "native" or "wasm": which engine answers. */
    this.engine = backend.kind;
    this.queue = Promise.resolve();
    this.live = 0;
  }

  /** True when the engine stopped and has not been restarted. */
  get crashed() {
    return !!this.backend.crashed;
  }

  /** One call, after every call made before it. */
  call(method, ...args) {
    const run = () =>
      this.backend.call(method, args).then((r) => {
        if (r && typeof r.live === "number") this.live = r.live;
        return r.value;
      });
    const p = this.queue.then(run, run);
    this.queue = p.catch(() => {});
    return p;
  }

  /** Bytes the core held after the last call. The native engine reports 0
   *  when it does not count. */
  liveBytes() {
    return this.live;
  }
  /** True when the engine has a memory ceiling worth warning about. */
  get bounded() {
    return this.engine === "wasm";
  }

  async restart() {
    await this.queue;
    await this.backend.restart();
  }

  open(bytes) {
    return this.call("open", bytes);
  }
  newDrawing(imperial) {
    return this.call("newDrawing", !!imperial);
  }
  /** The sample plate from the workspace mockups. */
  sample() {
    return this.call("sample");
  }
  close(id) {
    return this.call("close", id);
  }
  info(id) {
    return this.call("info", id);
  }
  async display(id) {
    return parseDisplay(toBytes(await this.call("display", id)));
  }
  /** What changed since the display was last read: `patch` lists the
   *  entities whose items it replaces, or it is the whole list. */
  async displayChanges(id) {
    return parseDisplay(toBytes(await this.call("displayChanges", id)));
  }
  /** Run a command line (crates/cad-ffi/src/session.rs); its JSON answer. */
  exec(id, line) {
    return this.call("exec", id, line);
  }
  /** Try a command; null instead of an error. A crash still throws. */
  async query(id, line) {
    try {
      return await this.exec(id, line);
    } catch (e) {
      if (e instanceof CoreCrash) throw e;
      return null;
    }
  }
  async save(id, format, user = "") {
    const r = await this.call("save", id, format, user);
    return { bytes: toBytes(r.bytes), notes: r.notes };
  }
}

/** Bytes from whatever a backend handed back (a view, a buffer, an array). */
function toBytes(b) {
  if (b instanceof Uint8Array) return b;
  if (b instanceof ArrayBuffer) return new Uint8Array(b);
  if (Array.isArray(b)) return Uint8Array.from(b);
  throw new Error("the core answered without bytes");
}

/**
 * Asks for the latest answer only: each call cancels interest in the ones
 * before it, so a slow reply can never land after a newer one (hover, snap
 * and pick run on every mouse move). Resolves to `stale` for an answer
 * that was overtaken.
 */
export function latestWins() {
  let seq = 0;
  const run = async (fn) => {
    const mine = ++seq;
    const v = await fn();
    return mine === seq ? v : latestWins.stale;
  };
  run.cancel = () => void ++seq;
  return run;
}
latestWins.stale = Symbol("stale");

/** The display list (crates/cad-core/src/export/display.rs) for drawing. */
export function parseDisplay(buf) {
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  const magic = dec.decode(buf.subarray(0, 4));
  if (magic !== "DSPL" || dv.getUint32(4, true) !== 1) throw new Error("not a display list");
  const n = dv.getUint32(8, true);
  const head = JSON.parse(dec.decode(buf.subarray(12, 12 + n)));
  const start = Math.ceil((12 + n) / 8) * 8;
  const ops = new Float64Array(buf.buffer.slice(buf.byteOffset + start, buf.byteOffset + buf.byteLength));
  // Items: where each starts and ends in `ops`, its entity and style.
  const items = [];
  let i = 0;
  while (i < ops.length) {
    const code = ops[i];
    if (code === 1) {
      items.push({ handle: ops[i + 1].toString(16).toUpperCase(), style: ops[i + 2], start: i + 3, end: i + 3, ops });
      i += 3;
      continue;
    }
    i += opLength(ops, i);
    items[items.length - 1].end = i;
  }
  const byHandle = new Map();
  items.forEach((it, k) => {
    if (!byHandle.has(it.handle)) byHandle.set(it.handle, []);
    byHandle.get(it.handle).push(k);
  });
  return { ...head, ops, items, byHandle };
}

/** Values in the operation at `i`, its code included. */
export function opLength(ops, i) {
  switch (ops[i]) {
    case 2: return 5;
    case 3: return 6;
    case 4: return 3 + 2 * ops[i + 1];
    case 5: {
      let j = i + 2;
      for (let r = 0; r < ops[i + 1]; r++) j += 1 + 2 * ops[j];
      return j - i;
    }
    case 6: return 3;
    case 7: return 7 + ops[i + 6];
    default: throw new Error(`display list: unknown operation ${ops[i]} at ${i}`);
  }
}
