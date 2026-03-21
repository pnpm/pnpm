import fs from 'fs'
import path from 'path'

const ownDir = import.meta.dirname
const placeholder = 'This file intentionally left blank'

for (const name of ['pnpm', 'pn', 'pnpx', 'pnx']) {
  const file = path.join(ownDir, name)
  try {
    fs.unlinkSync(file)
  } catch (e) {
    if (e.code !== 'ENOENT') throw e
  }
  fs.writeFileSync(file, placeholder, 'utf8')
}
