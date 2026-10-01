const fs = require('fs')

// Lets a test prove that an install restored the build from the side-effects cache.
if (process.env.PNPM_E2E_FAIL_POSTINSTALL) {
  console.error('postinstall ran although the build was expected to come from the side-effects cache')
  process.exit(1)
}

fs.mkdirSync('bin')
fs.writeFileSync('bin/tool', '#!/bin/sh\necho tool\n', { mode: 0o755 })
fs.mkdirSync('lib')
fs.writeFileSync('lib/index.js', 'module.exports = true\n')

if (process.platform !== 'win32') {
  fs.symlinkSync('tool', 'bin/tool-alias')
  fs.symlinkSync('lib', 'lib-link')
  fs.rmSync('replaced.js')
  fs.symlinkSync('lib/index.js', 'replaced.js')
}
