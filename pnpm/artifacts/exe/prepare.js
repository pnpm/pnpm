import fs from 'fs'
import path from 'path'

const ownDir = import.meta.dirname
const placeholder = 'This file intentionally left blank'

// pnpm is always replaced by setup.js (preinstall) or linkExePlatformBinary,
// so a dead placeholder is fine.
writeFile('pnpm', placeholder)

// pn/pnpx/pnx must be functional shell scripts so they work even when
// preinstall scripts are skipped (e.g. pnpm self-update uses ignoreScripts).
// setup.js will replace pn with a hardlink for better performance when it runs.
writeFile('pn', '#!/bin/sh\nexec "$(dirname "$0")/pnpm" "$@"\n', 0o755)
writeFile('pnpx', '#!/bin/sh\nexec "$(dirname "$0")/pnpm" dlx "$@"\n', 0o755)
writeFile('pnx', '#!/bin/sh\nexec "$(dirname "$0")/pnpm" dlx "$@"\n', 0o755)

function writeFile (name, content, mode) {
  const file = path.join(ownDir, name)
  try {
    fs.unlinkSync(file)
  } catch (e) {
    if (e.code !== 'ENOENT') throw e
  }
  fs.writeFileSync(file, content, { mode })
}
