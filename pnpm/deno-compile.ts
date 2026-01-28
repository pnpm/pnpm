/**
 * Compiles pnpm into a single executable using Deno.
 *
 * Prerequisites:
 *   1. Run `pnpm run compile` to build dist/pnpm.mjs and dist/worker.js
 *      (or `pnpm run _compile && pnpm run bundle`)
 *
 * Usage: deno run -A deno-compile.ts
 *
 * This produces a standalone binary at `./pnpm-deno`.
 */

const outFile = 'pnpm-deno'

// Node.js built-in modules that esbuild may leave as bare specifiers.
// Deno requires a "node:" prefix for these.
const NODE_BUILTINS = [
  'assert', 'assert/strict', 'async_hooks', 'buffer', 'child_process',
  'cluster', 'console', 'constants', 'crypto', 'dgram', 'diagnostics_channel',
  'dns', 'dns/promises', 'domain', 'events', 'fs', 'fs/promises', 'http',
  'http2', 'https', 'inspector', 'module', 'net', 'os', 'path', 'path/posix',
  'path/win32', 'perf_hooks', 'process', 'punycode', 'querystring', 'readline',
  'readline/promises', 'repl', 'stream', 'stream/consumers', 'stream/promises',
  'stream/web', 'string_decoder', 'sys', 'timers', 'timers/promises', 'tls',
  'trace_events', 'tty', 'url', 'util', 'util/types', 'v8', 'vm',
  'wasi', 'worker_threads', 'zlib',
]

async function main (): Promise<void> {
  // Verify prerequisites
  for (const f of ['dist/pnpm.mjs', 'dist/worker.js']) {
    try {
      await Deno.stat(f)
    } catch {
      console.error(`${f} not found. Run \`pnpm run compile\` first.`)
      Deno.exit(1)
    }
  }

  // Step 1: Patch dist files — add "node:" prefix and inject Deno fs acceleration.
  console.log('Patching dist files for Deno compatibility...')
  const patchedDir = '_deno_patched'
  try { await Deno.mkdir(patchedDir) } catch { /* already exists */ }
  for (const file of ['dist/pnpm.mjs', 'dist/worker.js']) {
    let code = await Deno.readTextFile(file)
    code = addNodePrefixes(code)
    code = injectFsAcceleration(code)
    await Deno.writeTextFile(`${patchedDir}/${file.split('/')[1]}`, code)
  }

  // Step 2: Read patched worker source to embed in the entry point.
  const workerCode = await Deno.readTextFile(`${patchedDir}/worker.js`)

  // Step 3: Compute content hash for persistent caching
  const hashBuf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(workerCode))
  const contentHash = Array.from(new Uint8Array(hashBuf)).map(b => b.toString(16).padStart(2, '0')).join('').substring(0, 12)

  // Step 4: Collect native .node addon files
  const nativeAddonLines = await generateNativeAddonExtraction()

  // Step 5: Create entry point
  const entrySource = `\
import { createRequire } from 'node:module';
import { Buffer } from 'node:buffer';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';

// Deno doesn't inject Node.js globals automatically. Shim them.
if (typeof globalThis.global === 'undefined') globalThis.global = globalThis;
if (typeof globalThis.Buffer === 'undefined') globalThis.Buffer = Buffer;

// Shim require on globalThis so eval('require') in @yarnpkg/fslib works.
globalThis.require = createRequire(import.meta.url);

process.setMaxListeners(0);
globalThis['pnpm__startedAt'] = Date.now();

// Use a persistent cache directory keyed by content hash.
// This avoids re-extracting the worker on every invocation.
const _cacheDir = path.join(os.tmpdir(), 'pnpm-deno-${contentHash}');
const _workerPath = path.join(_cacheDir, 'worker.js');
if (!fs.existsSync(_workerPath)) {
  const _workerCode = ${JSON.stringify(workerCode)};
  fs.mkdirSync(_cacheDir, { recursive: true });
  fs.writeFileSync(_workerPath, _workerCode);
${nativeAddonLines ? nativeAddonLines.split('\n').map(l => '  ' + l).join('\n') : ''}
}
process.env.PNPM_WORKER_PATH = _workerPath;

// Import the patched pnpm CLI bundle.
await import('./${patchedDir}/pnpm.mjs');
`

  const entryFile = '_deno_entry.mjs'
  await Deno.writeTextFile(entryFile, entrySource)

  // Step 6: Compile
  try {
    console.log('Compiling pnpm single executable with Deno...')
    const cmd = new Deno.Command('deno', {
      args: [
        'compile',
        '--allow-all',
        '--node-modules-dir=auto',
        `--output=${outFile}`,
        entryFile,
      ],
      stdout: 'inherit',
      stderr: 'inherit',
    })
    const result = await cmd.output()

    if (!result.success) {
      console.error('Compilation failed.')
      Deno.exit(1)
    }

    const stat = await Deno.stat(outFile)
    const sizeMB = ((stat.size ?? 0) / 1024 / 1024).toFixed(1)
    console.log(`\nSuccess! Binary created: ./${outFile}`)
    console.log(`Size: ${sizeMB} MB`)
    console.log(`\nTest it with: ./${outFile} --version`)
  } finally {
    try { await Deno.remove(entryFile) } catch { /* ignore */ }
    try { await Deno.remove(patchedDir, { recursive: true }) } catch { /* ignore */ }
  }
}

/**
 * Replace bare Node.js built-in imports with "node:" prefixed versions.
 * Handles both `from "fs"` and `require("fs")` patterns.
 */
function addNodePrefixes (code: string): string {
  for (const mod of NODE_BUILTINS) {
    // from "fs" → from "node:fs"  /  from 'fs' → from 'node:fs'
    code = code.replaceAll(`from "${mod}"`, `from "node:${mod}"`)
    code = code.replaceAll(`from '${mod}'`, `from 'node:${mod}'`)
    // require("fs") → require("node:fs")
    code = code.replaceAll(`require("${mod}")`, `require("node:${mod}")`)
    code = code.replaceAll(`require('${mod}')`, `require('node:${mod}')`)
    // Don't double-prefix: undo node:node:
  }
  code = code.replaceAll('node:node:', 'node:')
  return code
}

/**
 * Inject Deno native fs acceleration into an ESM bundle.
 *
 * Deno's `node:fs` compat shim adds a JavaScript layer that validates/normalizes
 * arguments before calling the underlying Rust ops. For hot paths (thousands of
 * linkSync, symlinkSync, statSync, etc. calls during `pnpm install`), this overhead
 * is measurable. By monkey-patching key `node:fs` methods to call `Deno.*` APIs
 * directly, we bypass the shim and go straight to the Rust ops.
 *
 * This is injected into both `pnpm.mjs` (main thread) and `worker.js` (worker
 * threads where the heavy linking/extraction happens).
 */
function injectFsAcceleration (code: string): string {
  // Find the insertion point: after all top-level `import` statements.
  const lines = code.split('\n')
  let insertIdx = 0
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trimStart()
    if (trimmed.startsWith('import ') || trimmed.startsWith('import{') || trimmed === '') {
      insertIdx = i + 1
    } else {
      break
    }
  }

  const accelCode = FS_ACCEL_CODE
  lines.splice(insertIdx, 0, accelCode)
  return lines.join('\n')
}

/**
 * Deno native fs acceleration code.
 *
 * This code monkey-patches `node:fs` synchronous methods to use Deno's native
 * APIs. Each patched method calls the Deno op directly (Rust FFI) instead of
 * going through Deno's JavaScript compatibility shim for Node.js.
 *
 * The shim overhead per call is small (~1-10μs), but during `pnpm install` with
 * thousands of packages, the cumulative effect is significant — particularly for
 * linkSync, symlinkSync, statSync, lstatSync, mkdirSync, readdirSync, etc.
 */
const FS_ACCEL_CODE = `
// --- Deno native fs acceleration ---
import _accelFs from 'node:fs';
(() => {
const _D = globalThis.Deno;
if (!_D) return;
const fs = _accelFs;

// Convert Deno FileInfo to Node.js Stats-compatible object.
function _wrapStat(info) {
  return {
    isFile: () => info.isFile,
    isDirectory: () => info.isDirectory,
    isSymbolicLink: () => info.isSymlink,
    isBlockDevice: () => false,
    isCharacterDevice: () => false,
    isFIFO: () => false,
    isSocket: () => false,
    size: info.size,
    mode: info.mode ?? 0o666,
    nlink: info.nlink ?? 1,
    uid: info.uid ?? 0,
    gid: info.gid ?? 0,
    dev: info.dev ?? 0,
    ino: info.ino ?? 0,
    rdev: info.rdev ?? 0,
    blksize: info.blksize ?? 4096,
    blocks: info.blocks ?? Math.ceil((info.size ?? 0) / 512),
    mtime: info.mtime,
    atime: info.atime,
    ctime: info.mtime,
    birthtime: info.birthtime,
    mtimeMs: info.mtime?.getTime() ?? 0,
    atimeMs: info.atime?.getTime() ?? 0,
    ctimeMs: info.mtime?.getTime() ?? 0,
    birthtimeMs: info.birthtime?.getTime() ?? 0,
    mtimeNs: info.mtime ? BigInt(info.mtime.getTime()) * 1000000n : 0n,
    atimeNs: info.atime ? BigInt(info.atime.getTime()) * 1000000n : 0n,
    ctimeNs: info.mtime ? BigInt(info.mtime.getTime()) * 1000000n : 0n,
    birthtimeNs: info.birthtime ? BigInt(info.birthtime.getTime()) * 1000000n : 0n,
  };
}

// Convert Deno DirEntry to Node.js Dirent-compatible object.
function _wrapDirent(entry, parentPath) {
  return {
    name: entry.name,
    path: parentPath,
    parentPath: parentPath,
    isFile: () => entry.isFile,
    isDirectory: () => entry.isDirectory,
    isSymbolicLink: () => entry.isSymlink,
    isBlockDevice: () => false,
    isCharacterDevice: () => false,
    isFIFO: () => false,
    isSocket: () => false,
  };
}

// --- Tier 1: Per-file operations (called thousands of times during linking) ---

const _origLinkSync = fs.linkSync;
fs.linkSync = function(a, b) { _D.linkSync(String(a), String(b)); };

const _origStatSync = fs.statSync;
fs.statSync = function(p, opts) {
  if (opts?.bigint) return _origStatSync(p, opts);
  try {
    return _wrapStat(_D.statSync(String(p)));
  } catch (e) {
    if (opts?.throwIfNoEntry === false && e?.code === 'ENOENT') return undefined;
    throw e;
  }
};

const _origLstatSync = fs.lstatSync;
fs.lstatSync = function(p, opts) {
  if (opts?.bigint) return _origLstatSync(p, opts);
  try {
    return _wrapStat(_D.lstatSync(String(p)));
  } catch (e) {
    if (opts?.throwIfNoEntry === false && e?.code === 'ENOENT') return undefined;
    throw e;
  }
};

const _origExistsSync = fs.existsSync;
fs.existsSync = function(p) {
  try { _D.lstatSync(String(p)); return true; } catch { return false; }
};

const _origMkdirSync = fs.mkdirSync;
fs.mkdirSync = function(p, opts) {
  const o = typeof opts === 'number' ? { mode: opts } : opts;
  try {
    _D.mkdirSync(String(p), o);
  } catch (e) {
    if (!(o?.recursive && e?.code === 'EEXIST')) throw e;
  }
  return undefined;
};

// --- Tier 2: Per-package operations ---

const _origSymlinkSync = fs.symlinkSync;
fs.symlinkSync = function(target, p, type) {
  _D.symlinkSync(String(target), String(p), type ? { type } : undefined);
};

const _origReaddirSync = fs.readdirSync;
fs.readdirSync = function(p, opts) {
  if (opts?.encoding === 'buffer') return _origReaddirSync(p, opts);
  const pStr = String(p);
  const entries = [];
  for (const e of _D.readDirSync(pStr)) {
    entries.push(opts?.withFileTypes ? _wrapDirent(e, pStr) : e.name);
  }
  return entries;
};

const _origUnlinkSync = fs.unlinkSync;
fs.unlinkSync = function(p) { _D.removeSync(String(p)); };

const _origRenameSync = fs.renameSync;
fs.renameSync = function(a, b) { _D.renameSync(String(a), String(b)); };

const _origReadlinkSync = fs.readlinkSync;
fs.readlinkSync = function(p, opts) {
  if (opts) return _origReadlinkSync(p, opts);
  return _D.readLinkSync(String(p));
};

// --- Tier 3: Less frequent but still beneficial ---

const _origCopyFileSync = fs.copyFileSync;
fs.copyFileSync = function(src, dest, mode) {
  if (mode) return _origCopyFileSync(src, dest, mode);
  _D.copyFileSync(String(src), String(dest));
};

const _origChmodSync = fs.chmodSync;
fs.chmodSync = function(p, mode) { _D.chmodSync(String(p), mode); };

const _origRmSync = fs.rmSync;
fs.rmSync = function(p, opts) {
  try {
    _D.removeSync(String(p), { recursive: !!opts?.recursive });
  } catch (e) {
    if (!(opts?.force && e?.code === 'ENOENT')) throw e;
  }
};

const _origRealpathSync = fs.realpathSync;
const _newRealpathSync = function(p, opts) {
  if (opts) return _origRealpathSync(p, opts);
  return _D.realPathSync(String(p));
};
_newRealpathSync.native = _origRealpathSync.native;
fs.realpathSync = _newRealpathSync;

const _origReadFileSync = fs.readFileSync;
fs.readFileSync = function(p, opts) {
  const enc = typeof opts === 'string' ? opts : opts?.encoding;
  if (typeof p !== 'string') return _origReadFileSync(p, opts);
  if (enc === 'utf8' || enc === 'utf-8') return _D.readTextFileSync(p);
  if (!enc && !opts?.flag) {
    const buf = globalThis.Buffer;
    return buf ? buf.from(_D.readFileSync(p)) : _origReadFileSync(p, opts);
  }
  return _origReadFileSync(p, opts);
};

const _origWriteFileSync = fs.writeFileSync;
fs.writeFileSync = function(p, data, opts) {
  if (typeof data === 'string' && typeof p === 'string') {
    const o = typeof opts === 'string' ? { encoding: opts } : opts;
    if (!o?.mode && !o?.flag) {
      _D.writeTextFileSync(p, data);
      return;
    }
  }
  return _origWriteFileSync(p, data, opts);
};
})();
// --- End Deno native fs acceleration ---
`

async function generateNativeAddonExtraction (): Promise<string> {
  const lines: string[] = []
  try {
    for await (const entry of Deno.readDir('dist')) {
      if (entry.name.endsWith('.node')) {
        const content = await Deno.readFile(`dist/${entry.name}`)
        const b64 = btoa(String.fromCharCode(...content))
        lines.push(
          `fs.writeFileSync(path.join(_cacheDir, ${JSON.stringify(entry.name)}), Buffer.from(${JSON.stringify(b64)}, 'base64'));`
        )
      }
    }
  } catch { /* dist/ may not exist or have no .node files */ }
  return lines.length > 0 ? '\n' + lines.join('\n') : ''
}

main()
