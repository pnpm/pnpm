import { readFile } from 'node:fs/promises'
import { endianness, homedir, hostname, tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { MessageChannel, Worker } from 'node:worker_threads'

import { WASIThreads } from '@emnapi/wasi-threads'

import { createGuestOperations } from './host/guest-operations.mjs'
import { cancelAtomicWaits, createAtomicWaits } from './host/atomic-waits.mjs'
import { createWasiFilesystem } from './host/filesystem.mjs'
import { createHostServices } from './host/operations.mjs'
import { attachWasi } from './host/wasi-imports.mjs'

export async function runWasm (file, options = {}) {
  const workerCount = options.workers ?? 4
  if (!Number.isSafeInteger(workerCount) || workerCount < 1) throw new RangeError('WASM worker count must be a positive integer')
  const memory = new WebAssembly.Memory({ initial: 256, maximum: 16384, shared: true })
  const { wasi, imports: filesystemImports, close: closeFilesystem, cancelPending: cancelFilesystemWaits, resolveDescriptor, acquireDescriptor } = createWasiFilesystem({
    version: 'preview1',
    args: options.args ?? [file],
    env: {
      HOME: homedir(),
      ...(options.env ?? process.env),
      PNPM_WASM_HOSTNAME: hostname(),
      PNPM_WASM_PLATFORM: process.platform,
      PNPM_WASM_ARCH: process.arch,
      PNPM_WASM_ENDIANNESS: endianness(),
      PNPM_WASM_CWD: process.cwd(),
      PNPM_WASM_PID: String(process.pid),
      PNPM_WASM_TMPDIR: tmpdir(),
      PNPM_WASM_EXECUTABLE: options.executable ? path.resolve(options.executable) : fileURLToPath(new URL('./pnpm.mjs', import.meta.url)),
      PNPM_WASM_RUNTIME: fileURLToPath(import.meta.url),
      PNPM_WASM_ARTIFACT: path.resolve(file),
    },
    preopens: options.preopens ?? { '/': '/' },
    returnOnExit: true,
  }, memory)
  const operations = createGuestOperations(createHostServices({ resolveDescriptor, acquireDescriptor }))
  const workers = []
  const atomicControls = []
  let fail
  const failed = new Promise((_resolve, reject) => { fail = reject })
  let finish
  const exited = new Promise(resolve => { finish = resolve })
  let closing = false
  let exitCode = 1
  const errors = []
  // The thread pool unrefs idle workers; promise completion alone cannot keep Node alive.
  const lifetime = new MessageChannel()
  lifetime.port1.start()
  lifetime.port1.ref()
  try {
    const threads = new WASIThreads({
      wasi,
      reuseWorker: { size: workerCount, strict: false },
      // New WebContainer workers need the supervisor to service imports during startup.
      waitThreadStart: false,
      beforeLoad (worker) {
        // Route loader errors through the run promise so cleanup can terminate every worker.
        worker.onerror = fail
      },
      onCreateWorker () {
        if (closing) throw new Error('WASM runtime is shutting down')
        const atomicControl = new SharedArrayBuffer(8)
        atomicControls.push(atomicControl)
        const worker = new Worker(new URL('./worker.mjs', import.meta.url), {
          execArgv: [], workerData: { atomicControl },
        })
        workers.push(worker)
        worker.on('error', fail)
        worker.on('message', message => {
          if (message.pnpmMainExit != null) finish(message.pnpmMainExit)
          if (message.pnpmMainError) fail(Object.assign(new Error(), message.pnpmMainError))
        })
        attachWasi(worker, { wasi_snapshot_preview1: wasi.wasiImport, pnpm_fs: filesystemImports })
        operations.attach(worker)
        return worker
      },
    })
    const module = await WebAssembly.compile(await readFile(file))
    const instance = await WebAssembly.instantiate(module, {
      env: { memory },
      wasi_snapshot_preview1: wasi.wasiImport,
      pnpm_fs: filesystemImports,
      pnpm_host: supervisorImports(module),
      pnpm_atomic: createAtomicWaits(memory, new SharedArrayBuffer(8)),
      ...threads.getImportObject(),
    })
    // Bind one descriptor table to the shared memory without running the guest on the supervisor.
    wasi.initialize({ exports: { memory, _initialize () {} } })
    threads.setup(instance, module, memory)
    await Promise.race([threads.preloadWorkers(), failed])
    runMain(threads, instance)
    exitCode = await Promise.race([exited, failed])
  } catch (error) {
    errors.push(error)
  } finally {
    closing = true
    try {
      await shutdown({ operations, workers, closeFilesystem, cancelFilesystemWaits, memory, atomicControls }, errors)
    } finally {
      lifetime.port1.close()
      lifetime.port2.close()
    }
  }
  if (errors.length === 1) throw errors[0]
  if (errors.length > 1) throw new AggregateError(errors, 'WASM runtime failed')
  return exitCode
}

async function shutdown (runtime, errors) {
  try {
    await runtime.operations.close()
  } catch (error) {
    errors.push(error)
  }
  runtime.cancelFilesystemWaits()
  for (const control of runtime.atomicControls) cancelAtomicWaits(runtime.memory, control)
  // The thread manager treats a worker exit it did not request as a crash and throws from its exit listener.
  for (const worker of runtime.workers) worker.removeAllListeners('exit')
  await Promise.all(runtime.workers.map(worker => worker.terminate())).catch(error => errors.push(error))
  // Imports remain serviced until termination so pending RPC and thread-spawn waits can finish.
  for (const worker of runtime.workers) worker.removeAllListeners('message')
  try {
    runtime.closeFilesystem()
  } catch (error) {
    errors.push(error)
  }
}

function supervisorImports (module) {
  return Object.fromEntries(WebAssembly.Module.imports(module)
    .filter(entry => entry.module === 'pnpm_host')
    .map(({ name }) => [name, () => { throw new Error(`Guest operation ${name} executed on supervisor`) }]))
}

function runMain (threads, instance) {
  const worker = threads.PThread.getNewWorker()
  if (!worker) throw new Error('Could not allocate the WASM main worker')
  worker.ref()
  worker.postMessage({ pnpmMainStart: true, tlsBase: instance.exports.__tls_base.value })
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const file = process.argv[2]
  if (!file) throw new Error('Usage: node run.mjs <module.wasm> [arguments...]')
  process.exitCode = await runWasm(file, { args: process.argv.slice(2) })
}
