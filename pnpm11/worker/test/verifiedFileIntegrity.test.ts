import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import { StoreIndex } from '@pnpm/store.index'

import {
  addFilesFromDir,
  finishWorkers,
  readPkgFromCafs,
  verifiedFileIntegritySince,
  verifiedFileIntegritySnapshot,
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

  const baseline = verifiedFileIntegritySnapshot()
  const result = await readPkgFromCafs({ storeDir, verifyStoreIntegrity: true }, filesIndexFile)
  const verified = verifiedFileIntegritySince(baseline)

  expect(result.verified).toBe(true)
  expect(verified.files).toBeGreaterThan(0)
  expect(verified.ms).toBeGreaterThan(0)

  // A later install diffs from its own baseline, so the reads above are
  // not charged to it a second time.
  const second = verifiedFileIntegritySnapshot()
  await readPkgFromCafs({ storeDir, verifyStoreIntegrity: false }, filesIndexFile)
  expect(verifiedFileIntegritySince(second)).toEqual({ files: 0, ms: 0 })

  storeIndex.close()
})

/** Every content file in the store, skipping the SQLite index. */
function storeFiles (storeDir: string): string[] {
  return fs.readdirSync(storeDir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && !entry.name.startsWith('index.db'))
    .map((entry) => path.join(entry.parentPath, entry.name))
}
