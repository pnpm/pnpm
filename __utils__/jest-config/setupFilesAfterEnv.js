import path from 'path'
import { finishWorkers } from '@pnpm/worker'

const pnpmBinDir = path.join(import.meta.dirname, 'node_modules/.bin')
process.env.PATH = `${pnpmBinDir}${path.delimiter}${process.env.PATH}`

afterAll(async () => {
  await finishWorkers()
})

// Diagnostic: dump active handles if the process is still alive after 5s.
// The timer is unref'd so it won't itself prevent exit.
setTimeout(() => {
  const handles = process._getActiveHandles()
  console.error(`\n[DIAGNOSTIC] Process still alive. Active handles: ${handles.length}`)
  for (const h of handles) {
    const info = {
      type: h.constructor.name,
      ref: typeof h.hasRef === 'function' ? h.hasRef() : undefined,
    }
    if (h.address) try { Object.assign(info, h.address()) } catch {}
    if (h._peername) info.peer = h._peername
    if (h.localPort) info.localPort = h.localPort
    if (h.remotePort) info.remotePort = h.remotePort
    if (h.remoteAddress) info.remoteAddress = h.remoteAddress
    console.error(`[DIAGNOSTIC]  - ${JSON.stringify(info)}`)
  }
  const reqs = process._getActiveRequests()
  if (reqs.length > 0) {
    console.error(`[DIAGNOSTIC] Active requests: ${reqs.length}`)
  }
}, 5000).unref()
