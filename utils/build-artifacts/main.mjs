import fs from 'fs'
import path from 'path'
import pkgRhel from 'pkg-rpm'
import { fileURLToPath } from 'url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

const artifactDir = path.join(__dirname, '../../packages/artifacts/linux-x64')
const pnpmDir = path.join(__dirname, '../../packages/pnpm')
const pnpmManifest = JSON.parse(fs.readFileSync(path.join(pnpmDir, 'package.json'), 'utf8'))

await pkgRhel({
  name: 'pnpm',
  version: pnpmManifest.version,
  dest: path.join(__dirname, 'dist'),
  src: pnpmDir,
  input: path.join(artifactDir, 'pnpm'),
  arch: 'x86_64',
  logger: console.log,
})
