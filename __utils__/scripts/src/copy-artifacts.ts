import fs from 'fs'
import * as execa from 'execa'
import path from 'path'
import makeEmptyDir from 'make-empty-dir'
import stream from 'stream'
import * as tar from 'tar'
import { glob } from 'tinyglobby'

const repoRoot = path.join(import.meta.dirname, '../../..')
const dest = path.join(repoRoot, 'dist')
const artifactsDir = path.join(repoRoot, 'pnpm/artifacts')

;(async () => {
  await makeEmptyDir(dest)
  if (!fs.existsSync(path.join(artifactsDir, 'linux-x64/pnpm'))) {
    execa.sync('pnpm', ['--filter=@pnpm/exe', 'run', 'prepublishOnly'], {
      cwd: repoRoot,
      stdio: 'inherit',
    })
  }
  await createArtifactTarball('linux-x64', 'pnpm')
  await createArtifactTarball('linuxstatic-x64', 'pnpm')
  await createArtifactTarball('linuxstatic-arm64', 'pnpm')
  await createArtifactTarball('linux-arm64', 'pnpm')
  await createArtifactTarball('macos-x64', 'pnpm')
  await createArtifactTarball('macos-arm64', 'pnpm')
  await createArtifactTarball('win-x64', 'pnpm.exe')
  await createArtifactTarball('win-arm64', 'pnpm.exe')
  await createSourceMapsArchive()
})()

async function createArtifactTarball (target: string, binaryName: string): Promise<void> {
  try {
    const artifactDir = path.join(artifactsDir, target)
    const binaryPath = path.join(artifactDir, binaryName)
    if (!fs.existsSync(binaryPath)) {
      console.log(`Warning: ${binaryPath} not found, skipping ${target}`)
      return
    }

    // Collect files to include in the tarball
    const filesToInclude = [binaryName]

    // Add dist/ directory contents
    const distDir = path.join(artifactDir, 'dist')
    if (fs.existsSync(distDir)) {
      const distFiles = await glob('**/*', { cwd: distDir, dot: true })
      for (const f of distFiles) {
        filesToInclude.push(path.join('dist', f))
      }
    }

    const isWindows = target.startsWith('win-')
    const archiveName = isWindows ? `pnpm-${target}.zip` : `pnpm-${target}.tar.gz`

    if (isWindows) {
      // Create zip for Windows
      const zipPath = path.join(dest, archiveName)
      execa.sync('zip', ['-r', zipPath, ...filesToInclude], {
        cwd: artifactDir,
        stdio: 'inherit',
      })
    } else {
      // Create tar.gz for Unix
      await stream.promises.pipeline(
        tar.create({ gzip: true, cwd: artifactDir }, filesToInclude),
        fs.createWriteStream(path.join(dest, archiveName))
      )
    }
    console.log(`Created ${archiveName}`)
  } catch (err) {
    console.log(err)
  }
}

async function createSourceMapsArchive () {
  const pnpmDistDir = path.join(repoRoot, 'pnpm/dist')

  // The tar.create function can accept a filter callback function, but this
  // approach ends up adding empty directories to the archive. Using tinyglobby
  // instead.
  const mapFiles = await glob('**/*.map', { cwd: pnpmDistDir })

  await stream.promises.pipeline(
    tar.create({ gzip: true, cwd: pnpmDistDir }, mapFiles),
    fs.createWriteStream(path.join(dest, 'source-maps.tgz'))
  )
}
