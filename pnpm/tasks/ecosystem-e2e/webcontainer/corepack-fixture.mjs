import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { promisify } from 'node:util'

const run = promisify(execFile)
const manifest = JSON.parse(fs.readFileSync('../pnpm/corepack/package/package.json', 'utf8'))
const corepack = path.resolve('../corepack-cli/node_modules/corepack/dist/corepack.js')
const env = {
  ...process.env,
  COREPACK_HOME: path.resolve('../corepack-home'),
  COREPACK_ENABLE_NETWORK: '0',
  COREPACK_DEFAULT_TO_LATEST: '0',
}
const installed = await run(process.execPath, [corepack, 'install', '--global', '--cache-only', '../pnpm/corepack-cache.tgz'], { env })
process.stdout.write(installed.stdout)
const project = JSON.parse(fs.readFileSync('package.json', 'utf8'))
project.packageManager = `pnpm@${manifest.version}`
fs.writeFileSync('package.json', JSON.stringify(project))
const selected = await run(process.execPath, [corepack, 'pnpm', '--version'], { env })
assert.equal(selected.stdout.trim(), manifest.version)
console.log('corepack-cached-wrapper-ok')
