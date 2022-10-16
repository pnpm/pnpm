import fs from 'fs'
import * as execa from 'execa'
import path from 'path'
import makeEmptyDir from 'make-empty-dir'

const repoRoot = path.join(__dirname, '../../..')
const dest = path.join(repoRoot, 'dist')
const artifactsDir = path.join(repoRoot, 'packages/artifacts')

;(async () => { // eslint-disable-line
  await makeEmptyDir(dest)
  if (!fs.existsSync(path.join(artifactsDir, 'linux-x64/pnpm'))) {
    execa.sync('pnpm', ['run', 'prepublishOnly', '--filter', '@pnpm/exe'], {
      cwd: repoRoot,
      stdio: 'inherit',
    })
  }
  copyArtifact('linux-x64/pnpm', 'pnpm-linux-x64')
  copyArtifact('linuxstatic-x64/pnpm', 'pnpm-linuxstatic-x64')
  copyArtifact('linuxstatic-arm64/pnpm', 'pnpm-linuxstatic-arm64')
  copyArtifact('linux-arm64/pnpm', 'pnpm-linux-arm64')
  copyArtifact('macos-x64/pnpm', 'pnpm-macos-x64')
  copyArtifact('macos-arm64/pnpm', 'pnpm-macos-arm64')
  copyArtifact('win-x64/pnpm.exe', 'pnpm-win-x64.exe')
})()

function copyArtifact (srcName: string, destName: string) {
  fs.copyFileSync(path.join(artifactsDir, srcName), path.join(dest, destName))
}
