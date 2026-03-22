// CJS → ESM bridge for Node.js SEA (Single Executable Application)
//
// In a SEA binary, relative import specifiers resolve against the *build-time*
// path of the embedded script, not the runtime location of the executable.
// We must resolve against process.execPath so the import works on any machine.
const { join, dirname } = require('path')
const { pathToFileURL } = require('url')

;(async () => {
  const distPath = join(dirname(process.execPath), 'dist', 'pnpm.mjs')
  await import(pathToFileURL(distPath).href)
})()
