import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import { StoreIndex } from '@pnpm/store.index'

import {
  addFilesFromDir,
  currentVerifiedFileIntegrity,
  finishWorkers,
  readPkgFromCafs,
  trackVerifiedFileIntegrity,
} from '../lib/index.js'

afterAll(() => finishWorkers())

// Verification runs in the workers, so the figures an install reports at
// the end only exist if every worker's share travels back with its
// response and is summed on the main thread.
test('files re-hashed while verifying the store are tallied on the main thread', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-verified-file-integrity-'))
  const dir = path.join(tmp, 'pkg')
  fs.mkdirSync(dir)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'tallied-pkg', version: '1.0.0' }))
  fs.writeFileSync(path.join(dir, 'index.js'), 'module.exports = "tallied"\n')
  const storeDir = path.join(tmp, 'store')
  const storeIndex = new StoreIndex(storeDir)
  const filesIndexFile = path.join(storeDir, 'tallied-pkg.json')

  await addFilesFromDir({ storeDir, dir, filesIndexFile, storeIndex })

  // Push the stored files' mtime past the `checkedAt` the index recorded
  // for them, which is what makes a read re-hash their content instead
  // of trusting the index.
  const future = new Date(Date.now() + 60_000)
  for (const file of storeFiles(storeDir)) {
    fs.utimesSync(file, future, future)
  }

  const verified = await trackVerifiedFileIntegrity(async () => {
    const result = await readPkgFromCafs({ storeDir, verifyStoreIntegrity: true }, filesIndexFile)
    expect(result.verified).toBe(true)
    return currentVerifiedFileIntegrity()
  })

  expect(verified.files).toBeGreaterThan(0)
  expect(verified.ms).toBeGreaterThan(0)

  // A second install's reads are its own: this one hashes nothing (the
  // store is left alone), and none of the first one's work follows it.
  const second = await trackVerifiedFileIntegrity(async () => {
    await readPkgFromCafs({ storeDir, verifyStoreIntegrity: false }, filesIndexFile)
    return currentVerifiedFileIntegrity()
  })
  expect(second).toEqual({ files: 0, ms: 0 })

  storeIndex.close()
})

// The scope is what keeps a recursive workspace command honest: its
// per-project installs overlap, and each reports the store verification
// it caused rather than its siblings'.
test('concurrent installs each see only their own verification', async () => {
  const verifying = await plantPackage('verifying')
  const trusting = await plantPackage('trusting')

  const future = new Date(Date.now() + 60_000)
  for (const store of [verifying, trusting]) {
    for (const file of storeFiles(store.storeDir)) {
      fs.utimesSync(file, future, future)
    }
  }

  // The trusting install samples only once the verifying one has
  // finished hashing. Without that barrier it could sample first and
  // read zeroes even from a shared tally, and the test would pass on
  // exactly the bug it exists to catch.
  let hashingDone!: () => void
  const hashed = new Promise<void>((resolve) => {
    hashingDone = resolve
  })

  const [verified, trusted] = await Promise.all([
    trackVerifiedFileIntegrity(async () => {
      await readPkgFromCafs({ storeDir: verifying.storeDir, verifyStoreIntegrity: true }, verifying.filesIndexFile)
      hashingDone()
      return currentVerifiedFileIntegrity()
    }),
    trackVerifiedFileIntegrity(async () => {
      await readPkgFromCafs({ storeDir: trusting.storeDir, verifyStoreIntegrity: false }, trusting.filesIndexFile)
      await hashed
      return currentVerifiedFileIntegrity()
    }),
  ])

  expect(verified.files).toBeGreaterThan(0)
  expect(trusted).toEqual({ files: 0, ms: 0 })

  verifying.storeIndex.close()
  trusting.storeIndex.close()
})

/** A store holding one package, ready to be read back. */
async function plantPackage (name: string): Promise<{ storeDir: string, filesIndexFile: string, storeIndex: StoreIndex }> {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `pnpm-verified-file-integrity-${name}-`))
  const dir = path.join(tmp, 'pkg')
  fs.mkdirSync(dir)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name, version: '1.0.0' }))
  fs.writeFileSync(path.join(dir, 'index.js'), `module.exports = "${name}"\n`)
  const storeDir = path.join(tmp, 'store')
  const storeIndex = new StoreIndex(storeDir)
  const filesIndexFile = path.join(storeDir, `${name}.json`)
  await addFilesFromDir({ storeDir, dir, filesIndexFile, storeIndex })
  return { storeDir, filesIndexFile, storeIndex }
}

/** Every content file in the store, skipping the SQLite index. */
function storeFiles (storeDir: string): string[] {
  return fs.readdirSync(storeDir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && !entry.name.startsWith('index.db'))
    .map((entry) => path.join(entry.parentPath, entry.name))
}
