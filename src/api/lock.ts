import path = require('path')
import thenify = require('thenify')
import lockfile = require('lockfile')
const lock = thenify(lockfile.lock)
const unlock = thenify(lockfile.unlock)

export default async function<T> (storePath: string, fn: () => Promise<T>): Promise<T> {
  const lockfile: string = path.resolve(storePath, 'lock')
  await lock(lockfile)
  try {
    const result = await fn()
    await unlock(lockfile)
    return result
  } catch (err) {
    await unlock(lockfile)
    throw err
  }
}
