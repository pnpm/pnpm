// CJS → ESM bridge for Node.js SEA (Single Executable Application)
//
// In a SEA binary, relative import specifiers resolve against the *build-time*
// path of the embedded script, not the runtime location of the executable.
// We must resolve against process.execPath so the import works on any machine.
//
// Goes through Module.createRequire() rather than the ambient require() or
// dynamic import(). In Node.js >=25.7, the ambient require() and import()
// inside a CJS SEA entry are replaced with embedder hooks that only know how
// to resolve built-in module names, so any attempt to load an external file
// fails with ERR_UNKNOWN_BUILTIN_MODULE. A createRequire() bound to the
// running binary returns a normal module loader that bypasses those hooks,
// and the pnpm bundle has no top-level await so synchronous require() of it
// (Node.js 22+ feature) loads cleanly.
const { join, dirname } = require('path')
const { createRequire } = require('module')

const distPath = join(dirname(process.execPath), 'dist', 'pnpm.mjs')
createRequire(process.execPath)(distPath)
