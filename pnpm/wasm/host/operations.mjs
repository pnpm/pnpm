import { request } from './network.mjs'
import { endStdin, spawnProcess, tryWaitProcess, waitProcess, writeStdin } from './process.mjs'
import { createResources } from './resources.mjs'
import { createSignals } from './signals.mjs'
import { spawnShell } from './shell.mjs'
import { createUpload, writeUpload } from './upload.mjs'
import { confirm, input, prompt } from './terminal.mjs'
import { openTerminal, writeTerminal } from './terminal-session.mjs'
import { password } from './terminal-password.mjs'

export function createHostServices (configuration = {}) {
  const resources = createResources()
  const signals = createSignals(configuration.signalSource)
  let closed = false
  return {
    async dispatch (message, options = {}) {
      if (closed) throw new Error('WASM host services are closed')
      options.signal?.throwIfAborted()
      switch (message.operation) {
      case 'network.request': return request(resources, message, options.signal)
      case 'upload.create': return createUpload(resources)
      case 'upload.write': return writeUpload(resources.get(message.handle, 'upload'), message.bytes)
      case 'upload.end': return resources.get(message.handle, 'upload').writer.close()
      case 'stream.read': return resources.get(message.handle, 'stream').read(message.maxBytes)
      case 'resource.close': return resources.close(message.handle)
      case 'process.spawn': return spawnProcess(resources, message, {
        signal: options.signal, acquireDescriptor: configuration.acquireDescriptor,
      })
      case 'shell.spawn': return spawnShell(resources, message, {
        signal: options.signal, acquireDescriptor: configuration.acquireDescriptor,
      })
      case 'process.write': return writeStdin(resources.get(message.handle, 'process'), message.bytes)
      case 'process.endDetached':
        resources.get(message.handle, 'process').child.stdin?.end()
        return null
      case 'process.closeStdin':
        resources.get(message.handle, 'process').child.stdin?.destroy()
        return null
      case 'process.end': return endStdin(resources.get(message.handle, 'process'))
      case 'process.wait': return waitProcess(resources.get(message.handle, 'process'))
      case 'process.tryWait': return tryWaitProcess(resources.get(message.handle, 'process'))
      case 'process.release':
        resources.get(message.handle, 'process')
        resources.remove(message.handle)
        return null
      case 'process.killPid':
        return resources.findProcess(message.pid).kill(message.signal ?? 'SIGTERM')
      case 'process.kill': return resources.get(message.handle, 'process').kill(message.signal ?? 'SIGTERM')
      case 'signal.next': return signals.next(options)
      case 'terminal.status': return { stdin: Boolean(process.stdin.isTTY), stdout: Boolean(process.stdout.isTTY), stderr: Boolean(process.stderr.isTTY) }
      case 'terminal.open': return openTerminal(resources)
      case 'terminal.readKey': return resources.get(message.handle, 'terminal').read(options.signal)
      case 'terminal.write': return writeTerminal(resources.get(message.handle, 'terminal'), message.text)
      case 'terminal.prompt': return prompt(message, options)
      case 'terminal.confirm': return confirm(message, options)
      case 'terminal.input': return input(message, options)
      case 'terminal.password': return password(message, options)
      default: throw new Error(`Unknown WASM host operation: ${message.operation}`)
      }
    },
    async close () {
      closed = true
      signals.close()
      await resources.closeAll()
    },
  }
}
