import fs from 'node:fs'
import path from 'node:path'

/**
 * Writes a `.bin` entry that prints `marker`, in the form a command lookup
 * finds on the current platform. Stands in for the shims an install links,
 * so a test can tell which bin directory a command was resolved from.
 */
export function writeFakeBin (binDir: string, name: string, marker: string): void {
  fs.mkdirSync(binDir, { recursive: true })
  fs.writeFileSync(path.join(binDir, `${name}.js`), `console.log(${JSON.stringify(marker)})\n`, 'utf8')
  if (process.platform === 'win32') {
    fs.writeFileSync(path.join(binDir, `${name}.cmd`), `@node "%~dp0${name}.js" %*\r\n`, 'utf8')
    return
  }
  const shim = path.join(binDir, name)
  fs.writeFileSync(shim, `#!/bin/sh\nexec node "$(dirname "$0")/${name}.js" "$@"\n`, 'utf8')
  fs.chmodSync(shim, 0o755)
}
