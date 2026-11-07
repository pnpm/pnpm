import fs from 'node:fs'
import path from 'node:path'
import { parentPort } from 'node:worker_threads'

import { hardLinkDir } from '@pnpm/fs.hard-link-dir'
import { symlinkDependencySync } from '@pnpm/fs.symlink-dependency'
import { takeVerifiedFileIntegrity } from '@pnpm/store.cafs'

import { addFilesFromDir, addTarballToStore } from './addToStore.js'
import { readPkgFromStoreIndex } from './readFromStore.js'
import { closeStoreIndexes, getCafsStore } from './storeCaches.js'
import type {
  AddDirToStoreMessage,
  HardLinkDirMessage,
  InitStoreMessage,
  LinkPkgMessage,
  ReadPkgFromCafsMessage,
  SymlinkAllModulesMessage,
  TarballExtractMessage,
} from './types.js'

export function startWorker (): void {
  process.on('uncaughtException', (err) => {
    console.error(err)
  })
  parentPort!.on('message', handleMessage)
}

type WorkerMessage =
  | TarballExtractMessage
  | LinkPkgMessage
  | AddDirToStoreMessage
  | ReadPkgFromCafsMessage
  | SymlinkAllModulesMessage
  | HardLinkDirMessage
  | InitStoreMessage

async function handleMessage (message: WorkerMessage | false): Promise<void> {
  if (message === false) {
    stopWorker()
  }
  try {
    parentPort!.postMessage(await processMessage(message))
  } catch (e: any) { // eslint-disable-line
    parentPort!.postMessage({
      status: 'error',
      // Drained here too: a request that hashed and then threw would
      // otherwise leave its share in this worker, to be handed to
      // whichever install asks next.
      verifiedFileIntegrity: takeVerifiedFileIntegrity(),
      error: {
        code: e.code,
        message: e.message ?? e.toString(),
        hint: e.hint,
      },
    })
  }
}

function stopWorker (): never {
  parentPort!.off('message', handleMessage)
  // Explicitly close cached SQLite connections before exiting.
  // process.exit() in a worker thread may not run C++ destructors,
  // which would leave file descriptors and mmap regions open.
  closeStoreIndexes()
  // eslint-disable-next-line n/no-process-exit -- in a worker thread this ends only the thread, which is how the pool retires a worker
  process.exit(0)
}

async function processMessage (message: WorkerMessage): Promise<object> {
  switch (message.type) {
    case 'extract':
      return addTarballToStore(message)
    case 'link':
      return importPackage(message)
    case 'add-dir':
      return addFilesFromDir(message)
    case 'init-store':
      return initStore(message)
    case 'readPkgFromCafs':
      return readPkgFromStoreIndex(message)
    case 'symlinkAllModules':
      return symlinkAllModules(message)
    case 'hardLinkDir':
      hardLinkDir(message.src, message.destDirs)
      return { status: 'success' }
  }
}

function initStore ({ storeDir }: InitStoreMessage): { status: string } {
  fs.mkdirSync(storeDir, { recursive: true })
  const hexChars = '0123456789abcdef'.split('')
  const filesDirPath = path.join(storeDir, 'files')
  try {
    fs.mkdirSync(filesDirPath)
  } catch {
    // If a parallel process has already started creating the directories in the store,
    // ignore if it already exists.
  }
  for (const hex1 of hexChars) {
    for (const hex2 of hexChars) {
      try {
        fs.mkdirSync(path.join(filesDirPath, `${hex1}${hex2}`))
      } catch {
        // If a parallel process has already started creating the directories in the store,
        // ignore if it already exists.
      }
    }
  }
  // The SQLite index database will be initialized lazily by getStoreIndex()
  // on the first operation that needs it (e.g., readPkgFromCafs, addFilesFromDir).
  // Eagerly opening it here races with the main thread's StoreIndex constructor,
  // which can cause SQLITE_CANTOPEN on Windows due to mandatory file locking.
  return { status: 'success' }
}

interface ImportPackageResult {
  status: string
  value: {
    isBuilt: boolean
    importMethod?: string
  }
}

function importPackage ({
  storeDir,
  packageImportMethod,
  filesResponse,
  sideEffectsCacheKey,
  targetDir,
  requiresBuild,
  force,
  keepModulesDir,
  disableRelinkLocalDirDeps,
  safeToSkip,
}: LinkPkgMessage): ImportPackageResult {
  const cafsStore = getCafsStore({ storeDir, packageImportMethod })
  const { importMethod, isBuilt } = cafsStore.importPackage(targetDir, {
    filesResponse,
    force,
    disableRelinkLocalDirDeps,
    requiresBuild,
    sideEffectsCacheKey,
    keepModulesDir,
    safeToSkip,
  })
  return { status: 'success', value: { isBuilt, importMethod } }
}

function symlinkAllModules (opts: SymlinkAllModulesMessage): { status: 'success' } {
  for (const dep of opts.deps) {
    for (const [alias, pkgDir] of Object.entries(dep.children)) {
      if (alias !== dep.name) {
        symlinkDependencySync(pkgDir, dep.modules, alias)
      }
    }
  }
  return { status: 'success' }
}
