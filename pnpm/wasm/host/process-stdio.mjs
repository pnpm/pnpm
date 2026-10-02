import { createReadStream, createWriteStream } from 'node:fs'
import { pipeline } from 'node:stream/promises'

export function prepareStdio (options, acquireDescriptor) {
  const redirects = new Map()
  try {
    const modes = ['stdin', 'stdout', 'stderr'].map((name, index) => {
      const mode = options[name] ?? 'pipe'
      if (mode && typeof mode === 'object' && Number.isInteger(mode.fd) && mode.fd >= 0) {
        if (!acquireDescriptor) throw new Error('WASI descriptor leasing is unavailable')
        const lease = acquireDescriptor(mode.fd)
        if (!Number.isInteger(lease.fd) || lease.fd < 0) throw new Error(`Invalid WASI descriptor: ${mode.fd}`)
        redirects.set(index, lease)
        return 'pipe'
      }
      if (!['pipe', 'inherit', 'ignore'].includes(mode)) throw new Error(`Invalid stdio mode: ${mode}`)
      return mode
    })
    return { modes, redirects, release: () => release(redirects) }
  } catch (error) {
    release(redirects)
    throw error
  }
}

export function bridgeDescriptors (child, redirects) {
  return Promise.allSettled([...redirects].map(async ([index, lease]) => {
    try {
      if (index === 0) await pipeline(createReadStream(null, { fd: lease.fd, autoClose: false }), child.stdin)
      else await pipeline(child.stdio[index], createWriteStream(null, { fd: lease.fd, autoClose: false }))
    } catch (error) {
      if (index !== 0 || !['EPIPE', 'ERR_STREAM_PREMATURE_CLOSE'].includes(error.code)) throw error
    } finally {
      lease.release()
    }
  })).then(results => results.find(result => result.status === 'rejected')?.reason)
}

function release (redirects) {
  for (const lease of redirects.values()) lease.release()
}
