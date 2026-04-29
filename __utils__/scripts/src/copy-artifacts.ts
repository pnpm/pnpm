import fs from 'node:fs'
import path from 'node:path'
import stream from 'node:stream'

import * as execa from 'execa'
import { makeEmptyDir } from 'make-empty-dir'
import * as tar from 'tar'
import { glob } from 'tinyglobby'

const repoRoot = path.join(import.meta.dirname, '../../..')
const dest = path.join(repoRoot, 'dist')
const artifactsDir = path.join(repoRoot, 'pnpm/artifacts')
const pnpmDistDir = path.join(repoRoot, 'pnpm/dist')

;(async () => {
  await makeEmptyDir(dest)
  if (!fs.existsSync(path.join(artifactsDir, 'linux-x64/pnpm'))) {
    execa.sync('pnpm', ['--filter=@pnpm/exe', 'run', 'build-artifacts'], {
      cwd: repoRoot,
      stdio: 'inherit',
    })
  }
  await createArtifactTarball('linux-x64', 'pnpm')
  await createArtifactTarball('linux-x64-musl', 'pnpm')
  await createArtifactTarball('linux-arm64', 'pnpm')
  await createArtifactTarball('linux-arm64-musl', 'pnpm')
  await createArtifactTarball('darwin-x64', 'pnpm')
  await createArtifactTarball('darwin-arm64', 'pnpm')
  await createArtifactTarball('win32-x64', 'pnpm.exe')
  await createArtifactTarball('win32-arm64', 'pnpm.exe')
  await createSourceMapsArchive()
})().catch((err) => {
  console.error(err)
  process.exitCode = 1
})

async function createArtifactTarball (target: string, binaryName: string): Promise<void> {
  try {
    const artifactDir = path.join(artifactsDir, target)
    const binaryPath = path.join(artifactDir, binaryName)
    if (!fs.existsSync(binaryPath)) {
      console.log(`Warning: ${binaryPath} not found, skipping ${target}`)
      return
    }

    // Copy dist/ from the pnpm build output and strip non-target reflink packages.
    // Source maps are removed from this copy — they are archived separately via
    // createSourceMapsArchive(), which reads from the original pnpmDistDir.
    const distDest = path.join(artifactDir, 'dist')
    fs.rmSync(distDest, { recursive: true, force: true })
    fs.cpSync(pnpmDistDir, distDest, { recursive: true })
    stripReflinkPackages(distDest, getReflinkKeepPackages(target))
    for (const mapFile of await glob('**/*.map', { cwd: distDest })) {
      fs.rmSync(path.join(distDest, mapFile))
    }

    const isWindows = target.startsWith('win32-')
    const archiveName = isWindows ? `pnpm-${target}.zip` : `pnpm-${target}.tar.gz`

    if (isWindows) {
      // Create zip for Windows
      const zipPath = path.join(dest, archiveName)
      execa.sync('zip', ['-r', zipPath, binaryName, 'dist'], {
        cwd: artifactDir,
        stdio: 'inherit',
      })
    } else {
      // Create tar.gz for Unix
      await stream.promises.pipeline(
        tar.create({ gzip: true, cwd: artifactDir }, [binaryName, 'dist']),
        fs.createWriteStream(path.join(dest, archiveName))
      )
    }
    console.log(`Created ${archiveName}`)
  } catch (err) {
    console.error(`Failed to create artifact for target "${target}":`, err)
    throw err
  }
}

async function createSourceMapsArchive () {
  // The tar.create function can accept a filter callback function, but this
  // approach ends up adding empty directories to the archive. Using tinyglobby
  // instead.
  const mapFiles = await glob('**/*.map', { cwd: pnpmDistDir })

  await stream.promises.pipeline(
    tar.create({ gzip: true, cwd: pnpmDistDir }, mapFiles),
    fs.createWriteStream(path.join(dest, 'source-maps.tgz'))
  )
}

// Reflink platform package names needed for a build target.
// Target format: '<platform>-<arch>[-<libc>]' where <platform> is one of
// 'linux', 'darwin', 'win32', <arch> is 'x64' | 'arm64', and <libc> is only
// ever 'musl' (linux-only).
function getReflinkKeepPackages (target: string): string[] {
  if (target.startsWith('darwin-')) {
    return [`@reflink/reflink-${target}`]
  }
  if (target.startsWith('win32-')) {
    return [`@reflink/reflink-${target}-msvc`]
  }
  if (target.startsWith('linux-')) {
    const arch = target.includes('arm64') ? 'arm64' : 'x64'
    return [
      `@reflink/reflink-linux-${arch}-gnu`,
      `@reflink/reflink-linux-${arch}-musl`,
    ]
  }
  return []
}

function stripReflinkPackages (distDir: string, keepPackages: string[]): void {
  const reflinkDir = path.join(distDir, 'node_modules', '@reflink')
  if (!fs.existsSync(reflinkDir)) return

  for (const entry of fs.readdirSync(reflinkDir)) {
    if (entry === 'reflink') continue // keep the main package
    if (!keepPackages.includes(`@reflink/${entry}`)) {
      fs.rmSync(path.join(reflinkDir, entry), { recursive: true })
    }
  }
}
