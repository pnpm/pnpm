import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

const importerFixture = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  'fixtures/concurrentSharedSlotImport.mjs'
)

test('a concurrent importer repairs a partial shared virtual-store slot before marking it complete', async () => {
  const tmp = tempDir()
  const srcDir = path.join(tmp, 'src')
  const newDir = path.join(tmp, 'slot')
  const readyFile = path.join(tmp, 'writer-ready')
  const releaseFile = path.join(tmp, 'release-writer')
  fs.mkdirSync(srcDir)
  fs.writeFileSync(path.join(srcDir, 'index.js'), 'module.exports = 1')
  fs.writeFileSync(path.join(srcDir, 'package.json'), '{"name":"pkg"}')

  // Separate installs may select different import tiers while sharing the GVS.
  // Model a copy interrupted mid-file, then let a hardlink importer encounter
  // that occupied dirent before the first process can place package.json.
  const writer = spawn(process.execPath, importerArgs('partial-writer'), { stdio: 'inherit' })
  const writerExit = once(writer, 'exit')
  try {
    await Promise.race([
      waitForFile(readyFile),
      writerExit.then(() => {
        if (!fs.existsSync(readyFile)) throw new Error('partial writer exited before reaching the barrier')
      }),
    ])
    execFileSync(process.execPath, importerArgs('contender'), { stdio: 'pipe', timeout: 20_000 })
  } finally {
    fs.writeFileSync(releaseFile, '')
    writer.kill()
    await writerExit
  }

  expect(fs.readFileSync(path.join(newDir, 'index.js'), 'utf8')).toBe('module.exports = 1')
  expect(fs.readFileSync(path.join(newDir, 'package.json'), 'utf8')).toBe('{"name":"pkg"}')
  expect(fs.readFileSync(path.join(newDir, 'writer-owned.txt'), 'utf8')).toBe(
    'the first importer is still using this directory'
  )

  function importerArgs (role: 'partial-writer' | 'contender'): string[] {
    return [importerFixture, role, newDir, srcDir, readyFile, releaseFile]
  }
}, 30_000)

function waitForFile (file: string): Promise<void> {
  const deadline = Date.now() + 10_000
  return new Promise((resolve, reject) => {
    poll()

    function poll (): void {
      if (fs.existsSync(file)) {
        resolve()
      } else if (Date.now() >= deadline) {
        reject(new Error(`timed out waiting for ${file}`))
      } else {
        setTimeout(poll, 10)
      }
    }
  })
}
