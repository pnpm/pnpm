import fs from 'fs'
import path from 'path'
import pkgDeb from 'pkg-deb'
import pkgRhel from 'pkg-rpm'
import { fileURLToPath } from 'url'

const artifactDir = path.join(import.meta.dirname, '../../packages/artifacts/linux-x64')
const pnpmDir = path.join(import.meta.dirname, '../../packages/pnpm')
const pnpmManifest = JSON.parse(fs.readFileSync(path.join(pnpmDir, 'package.json'), 'utf8'))

const opts = {
  name: 'pnpm',
  version: pnpmManifest.version,
  dest: path.join(import.meta.dirname, '../../dist'),
  src: pnpmDir,
  input: path.join(artifactDir, 'pnpm'),
  arch: 'x64',
  logger: console.log,
}

if (process.argv.includes('rpm')) await pkgRhel(opts)
if (process.argv.includes('deb')) await pkgDeb(opts)

