import path from 'path'
import { finishWorkers } from '@pnpm/worker'

const pnpmBinDir = path.join(import.meta.dirname, 'node_modules/.bin')
process.env.PATH = `${pnpmBinDir}${path.delimiter}${process.env.PATH}`

afterAll(async () => {
  await finishWorkers()
  // Diagnostic: dump active handles if the process is still alive 5s after finishWorkers.
  // The timer is unref'd so it won't itself prevent exit.
  setTimeout(() => {
    const handles = process._getActiveHandles()
    const refHandles = handles.filter(h => typeof h.hasRef !== 'function' || h.hasRef())
    console.error(`\n[DIAGNOSTIC] Process still alive after finishWorkers. Ref'd handles: ${refHandles.length} (total: ${handles.length})`)
    for (const h of refHandles) {
      const info = { type: h.constructor.name }
      if (typeof h.hasRef === 'function') info.ref = h.hasRef()
      if (h.localAddress) info.localAddress = h.localAddress
      if (h.localPort) info.localPort = h.localPort
      if (h.remoteAddress) info.remoteAddress = h.remoteAddress
      if (h.remotePort) info.remotePort = h.remotePort
      if (h.remoteFamily) info.remoteFamily = h.remoteFamily
      if (h._peername) info.peer = h._peername
      if (h.address && typeof h.address === 'function') try { info.address = h.address() } catch {}
      if (h.readyState) info.readyState = h.readyState
      if (h.destroyed != null) info.destroyed = h.destroyed
      if (h.connecting != null) info.connecting = h.connecting
      if (h.pending != null) info.pending = h.pending
      if (h.pid != null) info.pid = h.pid
      if (h.exitCode != null) info.exitCode = h.exitCode
      if (h.signalCode != null) info.signalCode = h.signalCode
      if (h.killed != null) info.killed = h.killed
      if (h.spawnfile) info.spawnfile = h.spawnfile
      if (h.spawnargs) info.spawnargs = h.spawnargs
      console.error(`[DIAGNOSTIC]  - ${JSON.stringify(info)}`)
    }
  }, 5000).unref()
})
