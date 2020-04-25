import rimraf = require('@zkochan/rimraf')
import fs = require('mz/fs')
import pLimit from 'p-limit'
import ssri = require('ssri')
import { getFilePathInCafs } from '.'

const limit = pLimit(20)
const MAX_BULK_SIZE = 1 * 1024 * 1024 // 1MB

export default async function (
  cafsDir: string,
  integrityObj: Record<string, { size: number, integrity: string }>,
) {
  let verified = true
  await Promise.all(
    Object.keys(integrityObj)
      .map((f) =>
        limit(async () => {
          const fstat = integrityObj[f]
          if (!fstat.integrity) {
            throw new Error(`Integrity checksum is missing for ${f}`)
          }
          if (
            !await verifyFile(
              getFilePathInCafs(cafsDir, fstat.integrity),
              fstat,
            )
          ) {
            verified = false
          }
        }),
      ),
  )
  return verified
}

async function verifyFile (filename: string, fstat: { size: number, integrity: string }) {
  if (fstat.size > MAX_BULK_SIZE) {
    try {
      const ok = Boolean(await ssri.checkStream(fs.createReadStream(filename), fstat.integrity))
      if (!ok) {
        await rimraf(filename)
      }
      return ok
    } catch (err) {
      switch (err.code) {
        case 'ENOENT': return false
        case 'EINTEGRITY': {
          // Broken files are removed from the store
          await rimraf(filename)
          return false
        }
      }
      throw err
    }
  }

  try {
    const data = await fs.readFile(filename)
    const ok = Boolean(ssri.checkData(data, fstat.integrity))
    if (!ok) {
      await rimraf(filename)
    }
    return ok
  } catch (err) {
    switch (err.code) {
      case 'ENOENT': return false
      case 'EINTEGRITY': {
        // Broken files are removed from the store
        await rimraf(filename)
        return false
      }
    }
    throw err
  }
}
