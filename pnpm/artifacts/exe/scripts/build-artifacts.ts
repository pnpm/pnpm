import * as execa from 'execa'
import fs from 'fs'
import path from 'path'

const artifactsDir = path.join(__dirname, '../..')

function build (target: string) {
  let artifactFile = path.join(artifactsDir, target, 'pnpm')
  if (target.startsWith('win-')) {
    artifactFile += '.exe'
  }
  try {
    fs.unlinkSync(artifactFile)
  } catch (err) {}
  execa.sync('pkg', ['../../dist/pnpm.cjs', `--config=../../package-${target}.json`], {
    cwd: path.join(__dirname, '..'),
    stdio: 'inherit',
  })
  // Verifying that the artifact was created.
  fs.statSync(artifactFile);
}

build('win-x64')
build('linux-x64')
build('linuxstatic-x64')
build('macos-x64')

const isM1Mac = process.platform === 'darwin' && process.arch === 'arm64'
if (process.platform === 'linux' || isM1Mac) {
  build('macos-arm64')
  build('linux-arm64')
  build('linuxstatic-arm64')
}

