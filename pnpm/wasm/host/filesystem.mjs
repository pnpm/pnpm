import fs from 'node:fs'
import { Buffer } from 'node:buffer'
import path from 'node:path'
import process from 'node:process'
import { TextDecoder } from 'node:util'

import { WASI } from '@tybys/wasm-util'

import { createFileLocks } from './file-locks.mjs'
import { hasExecutableMode } from './executable-mode.mjs'
import { createPoll } from './poll.mjs'
import { errno } from './wasi-errno.mjs'

const readRights = 2n | 4n | 16n | 32n | 2097152n
const writeRights = readRights | 1n | 64n | 4194304n
// Node does not expose Linux O_PATH through fs.constants.
const linuxOPath = 0o10000000

export function createWasiFilesystem (options, memory) {
  const locks = createFileLocks()
  const leases = createDescriptorLeases(locks)
  const sqliteFiles = new Map()
  const randomDevices = new Set()
  const descriptors = new Map([[0, 0], [1, 1], [2, 2]])
  const rootDescriptor = Object.keys(options.preopens ?? {}).indexOf('/') + 3
  const state = { leases, sqliteFiles, randomDevices, descriptors, constructing: true, nextPreopen: 3 }
  const wasi = new WASI({ ...options, fs: createFilesystemAdapter(state) })
  const polling = createPoll(memory)
  wasi.wasiImport.poll_oneoff = polling.poll
  state.constructing = false
  const open = trackPathOpens(wasi, state, memory)
  trackAbsoluteSymlinks(wasi, state, { options, memory })
  trackDescriptorChanges(wasi, descriptors)
  function resolveDescriptor (descriptor) {
    const native = descriptors.get(descriptor)
    if (native == null) throw Object.assign(new Error('Unknown WASI file descriptor'), { code: 'EBADF' })
    return native
  }
  const imports = createFilesystemImports({ memory, resolveDescriptor, open, rootDescriptor, options, locks, sqliteFiles })
  return { wasi, imports, resolveDescriptor, cancelPending: polling.close, acquireDescriptor (descriptor) {
    return leases.acquire(resolveDescriptor(descriptor))
  }, close () {
    polling.close()
    for (const descriptor of descriptors.keys()) {
      if (descriptor > 2) wasi.wasiImport.fd_close(descriptor)
    }
  } }
}

function trackPathOpens (wasi, state, memory) {
  const originalOpen = wasi.wasiImport.path_open
  function open (args, mode) {
    state.opening = { mode, follow: (args[1] & 1) !== 0 }
    try {
      new DataView(memory.buffer).getUint32(args[8] >>> 0, true)
      const result = originalOpen(...args)
      if (result === 0) {
        const descriptor = new DataView(memory.buffer).getUint32(args[8], true)
        state.descriptors.set(descriptor, state.opening.descriptor)
      }
      return result
    } catch (error) {
      return errorNumber(error)
    } finally {
      state.opening = undefined
    }
  }
  wasi.wasiImport.path_open = (...args) => open(args)
  return open
}

function trackAbsoluteSymlinks (wasi, state, { options, memory }) {
  const symlink = wasi.wasiImport.path_symlink
  wasi.wasiImport.path_symlink = (target, length, descriptor, link, linkLength) => {
    if (options.preopens?.['/'] !== '/' || new Uint8Array(memory.buffer)[target >>> 0] !== 47) {
      return symlink(target, length, descriptor, link, linkLength)
    }
    try {
      state.symlinkTarget = new TextDecoder('utf-8', { fatal: true }).decode(Uint8Array.from(new Uint8Array(memory.buffer, target >>> 0, length >>> 0)))
      return symlink(target + 1, length - 1, descriptor, link, linkLength)
    } catch (error) {
      return errorNumber(error)
    } finally {
      state.symlinkTarget = undefined
    }
  }
}

function createFilesystemAdapter (state) {
  const { leases, sqliteFiles, randomDevices, descriptors } = state
  return {
    ...fs,
    closeSync (descriptor) {
      randomDevices.delete(descriptor)
      leases.close(descriptor)
    },
    readSync (descriptor, buffer, offset, length, position) {
      if (!randomDevices.has(descriptor)) return fs.readSync(descriptor, buffer, offset, length, position)
      // WebContainer random devices use WebCrypto, which rejects shared views.
      const temporary = Buffer.alloc(Math.min(length, 65536))
      const count = fs.readSync(descriptor, temporary, 0, temporary.length, position)
      buffer.set(temporary.subarray(0, count), offset)
      return count
    },
    openSync (file, flags, mode) {
      if (state.opening && !state.opening.follow) flags |= fs.constants.O_NOFOLLOW
      const database = sqliteFiles.get(file)
      const sqliteMode = database ? fs.statSync(database).mode & 0o777 : undefined
      const descriptor = fs.openSync(file, flags, state.opening?.mode ?? sqliteMode ?? mode)
      if (file === '/dev/urandom' || file === '/dev/random') randomDevices.add(descriptor)
      if (state.constructing) descriptors.set(state.nextPreopen++, descriptor)
      else if (state.opening) state.opening.descriptor = descriptor
      return descriptor
    },
    symlinkSync (target, file, ...args) {
      fs.symlinkSync(state.symlinkTarget ?? target, file, ...args)
    },
    readlinkSync (file, ...args) {
      const target = fs.readlinkSync(file, ...args)
      return state.opening?.follow ? path.resolve(path.dirname(file), target) : target
    },
  }
}

function createDescriptorLeases (locks) {
  const active = new Map()
  function close (descriptor) {
    const state = active.get(descriptor)
    if (state) {
      state.closed = true
      return
    }
    fs.closeSync(descriptor)
    locks.release(descriptor)
  }
  return { close, acquire (descriptor) {
    let state = active.get(descriptor)
    if (!state) {
      state = { count: 0, closed: false }
      active.set(descriptor, state)
    }
    state.count++
    let released = false
    return { fd: descriptor, release () {
      if (released) return
      released = true
      if (--state.count !== 0) return
      active.delete(descriptor)
      if (state.closed) close(descriptor)
    } }
  } }
}

function trackDescriptorChanges (wasi, descriptors) {
  const close = wasi.wasiImport.fd_close
  wasi.wasiImport.fd_close = descriptor => {
    if (!descriptors.has(descriptor)) return errno.EBADF
    const result = close(descriptor)
    if (result === 0) {
      descriptors.delete(descriptor)
    }
    return result
  }
  const renumber = wasi.wasiImport.fd_renumber
  wasi.wasiImport.fd_renumber = (source, destination) => {
    if (!descriptors.has(source) || !descriptors.has(destination)) return errno.EBADF
    const result = renumber(source, destination)
    if (result === 0 && source !== destination) {
      descriptors.set(destination, descriptors.get(source))
      descriptors.delete(source)
    }
    return result
  }
}

function createFilesystemImports ({ memory, resolveDescriptor, open, rootDescriptor, options, locks, sqliteFiles }) {
  function guestPath (pointer, length) {
    const value = new TextDecoder('utf-8', { fatal: true }).decode(Uint8Array.from(new Uint8Array(memory.buffer, pointer >>> 0, length >>> 0)))
    if (!value.startsWith('/') || value.includes('\0')) throw Object.assign(new Error('Expected an absolute guest path'), { code: 'EINVAL' })
    const root = options.preopens?.['/']
    if (root !== '/') throw Object.assign(new Error('Filesystem extensions require the host root preopen'), { code: 'EACCES' })
    return path.resolve(root, `.${value}`)
  }
  function descendantComponents (directory, template) {
    const suffix = path.relative(template, directory)
    if (!suffix || suffix === '..' || suffix.startsWith(`..${path.sep}`) || path.isAbsolute(suffix)) {
      throw Object.assign(new Error('Permission target is not below its template'), { code: 'EINVAL' })
    }
    return suffix.split(path.sep)
  }
  return {
    ...createOpenImports({ options, rootDescriptor, open }),
    open_directory_nofollow_beneath (pointer, length, templatePointer, templateLength, output) {
      try {
        const directory = guestPath(pointer, length)
        const template = guestPath(templatePointer, templateLength)
        let current = template
        for (const component of descendantComponents(directory, template)) {
          current = path.join(current, component)
          const metadata = fs.lstatSync(current)
          if (metadata.isSymbolicLink()) return errno.ELOOP
          if (!metadata.isDirectory()) return errno.ENOTDIR
        }
        return open([rootDescriptor, 0, pointer, length, 0, readRights, 0n, 4, output])
      } catch (error) {
        return errorNumber(error)
      }
    },
    grant_directory_mode_beneath (pointer, length, templatePointer, templateLength, extra) {
      if ('webcontainer' in process.versions) return errno.ENOTSUP
      if (process.platform !== 'linux' || !fs.existsSync('/proc/self/fd')) return errno.ENOTSUP
      if (extra < 0 || extra > 0o7777) return errno.EINVAL
      try {
        const directory = guestPath(pointer, length)
        const template = guestPath(templatePointer, templateLength)
        const components = descendantComponents(directory, template)
        let descriptor = fs.openSync(template, linuxOPath | fs.constants.O_DIRECTORY)
        try {
          for (const component of components) {
            const child = fs.openSync(`/proc/self/fd/${descriptor}/${component}`, linuxOPath | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW)
            fs.closeSync(descriptor)
            descriptor = child
          }
          const metadata = fs.fstatSync(descriptor)
          if (metadata.uid !== process.getuid()) return errno.EACCES
          const current = metadata.mode & 0o7777
          if ((current | extra) !== current) fs.chmodSync(`/proc/self/fd/${descriptor}`, current | extra)
          return 0
        } finally {
          fs.closeSync(descriptor)
        }
      } catch (error) {
        return errorNumber(error)
      }
    },
    try_lock (descriptor, exclusive) {
      try { return locks.tryLock(resolveDescriptor(descriptor), exclusive !== 0) } catch (error) { return errorNumber(error) }
    },
    sqlite_register: checked((pointer, length, registered) => {
      const database = guestPath(pointer, length)
      for (const suffix of ['-journal', '-wal', '-shm']) {
        const file = database + suffix
        if (registered) sqliteFiles.set(file, database)
        else sqliteFiles.delete(file)
      }
    }),
    path_access_executable: checked((pointer, length) => executableAccess(guestPath(pointer, length))),
    check_owner: checked(descriptor => {
      if (fs.fstatSync(resolveDescriptor(descriptor)).uid !== process.getuid()) {
        throw Object.assign(new Error('WASM SQLite stores must belong to the current user'), { code: 'EACCES' })
      }
    }),
    fchmod: checked((descriptor, mode) => fs.fchmodSync(resolveDescriptor(descriptor), mode)),
    fmode: checked((descriptor, output) => {
      new DataView(memory.buffer).setUint32(output >>> 0, fs.fstatSync(resolveDescriptor(descriptor)).mode, true)
    }),
    lmode: checked((pointer, length, output) => {
      new DataView(memory.buffer).setUint32(output >>> 0, fs.lstatSync(guestPath(pointer, length)).mode, true)
    }),
    umask () { return process.umask() },
    uid () { return process.getuid() },
    secure_directory: checked((pointer, length) => secureDirectory(guestPath(pointer, length))),
  }
}

function executableAccess (file) {
  fs.accessSync(file, fs.constants.X_OK)
  // WebContainer accessSync ignores X_OK, although stat exposes permission bits.
  if (!hasExecutableMode(fs.statSync(file))) {
    throw Object.assign(new Error(`File is not executable: ${file}`), { code: 'EACCES' })
  }
}

function secureDirectory (directory) {
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 })
  const descriptor = fs.openSync(directory, fs.constants.O_RDONLY | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW)
  try {
    const metadata = fs.fstatSync(descriptor)
    if (metadata.uid !== process.getuid()) throw Object.assign(new Error('Lock directory has a different owner'), { code: 'EACCES' })
    fs.fchmodSync(descriptor, 0o700)
  } finally {
    fs.closeSync(descriptor)
  }
}

function createOpenImports ({ options, rootDescriptor, open }) {
  return {
    create_new (pointer, length, mode, output) {
      if (mode < 0 || mode > 0o7777) return errno.EINVAL
      if (options.preopens?.['/'] !== '/') return errno.EACCES
      return open([rootDescriptor, 0, pointer, length, 1 | 4, writeRights, 0n, 0, output], mode)
    },
    open_nofollow (pointer, length, output) {
      if (options.preopens?.['/'] !== '/') return errno.EACCES
      return open([rootDescriptor, 0, pointer, length, 0, readRights, 0n, 4, output])
    },
    open_lock (pointer, length, output) {
      if (options.preopens?.['/'] !== '/') return errno.EACCES
      return open([rootDescriptor, 0, pointer, length, 0, writeRights, 0n, 4, output])
    },
  }
}

function checked (operation) {
  return (...args) => {
    try {
      operation(...args)
      return 0
    } catch (error) {
      return errorNumber(error)
    }
  }
}

function errorNumber (error) {
  if (error instanceof RangeError) return errno.EFAULT
  return errno[error.code] ?? errno.EIO
}
