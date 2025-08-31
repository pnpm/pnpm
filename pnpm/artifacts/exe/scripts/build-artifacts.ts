import * as execa from 'execa'
import fs from 'fs'
import path from 'path'

const artifactsDir = path.join(import.meta.dirname, '../..')

function build (target: string) {
  let artifactFile = path.join(artifactsDir, target, 'pnpm')
  if (target.startsWith('win-')) {
    artifactFile += '.exe'
  }
  try {
    fs.unlinkSync(artifactFile)
  } catch {}
  execa.sync('pkg', ['../../pnpm.mjs', `--config=../../package-${target}.json`, '--sea'], {
    cwd: path.join(import.meta.dirname, '..'),
    stdio: 'inherit',
  })
  // Verifying that the artifact was created.
  fs.statSync(artifactFile);
}

build('win-x64')
build('linux-x64')
build('macos-x64')

const isM1Mac = process.platform === 'darwin' && process.arch === 'arm64'
if (process.platform === 'linux' || isM1Mac) {
  build('macos-arm64')
  build('linux-arm64')
  build('win-arm64')
}

