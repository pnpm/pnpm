import { Buffer } from 'node:buffer'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export function loaderError (code, message) {
  return Object.assign(new Error(message), { code })
}

export function openStore (manifestURL) {
  const manifestPath = fileURLToPath(manifestURL)
  const base = path.dirname(manifestPath)
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
  if (!isRecord(manifest) || manifest.version !== 1 || typeof manifest.storeDir !== 'string') {
    throw loaderError('ERR_PNPM_LOADER_MANIFEST', `Invalid store manifest: ${manifestPath}`)
  }
  const virtualRoot = path.join(base, '.pnpm-loader')
  const storeDir = path.resolve(base, manifest.storeDir)
  const packages = readPackages(manifest.packages, base, virtualRoot)
  const files = new Map()
  const directories = new Set([virtualRoot])
  for (const pkg of packages.values()) indexFiles(pkg, files, directories, storeDir)
  for (const filename of files.keys()) {
    if (directories.has(filename)) throw loaderError('ERR_PNPM_LOADER_MANIFEST', `File is also a directory: ${filename}`)
  }
  const owner = packageLookup(packages)
  const filesystem = virtualFilesystem({ files, directories, virtualRoot })
  return { packages, owner, filesystem, files, virtualRoot }
}

function packageLookup (packages) {
  const roots = new Map([...packages.values()].map(pkg => [pkg.root, pkg]))
  const cache = new Map()
  return function owner (filename) {
    if (cache.has(filename)) return cache.get(filename)
    let directory = filename
    while (!roots.has(directory) && path.dirname(directory) !== directory) {
      directory = path.dirname(directory)
    }
    const pkg = roots.get(directory)
    cache.set(filename, pkg)
    return pkg
  }
}

function readPackages (entries, base, virtualRoot) {
  if (!isRecord(entries)) {
    throw loaderError('ERR_PNPM_LOADER_MANIFEST', 'The store manifest needs a packages object')
  }
  const packages = new Map()
  const roots = new Set()
  for (const [id, entry] of Object.entries(entries)) {
    validatePackage(id, entry)
    const stored = Object.hasOwn(entry, 'files')
    if (stored === Object.hasOwn(entry, 'root')) {
      throw loaderError('ERR_PNPM_LOADER_MANIFEST', `${id} must have either files or root`)
    }
    const root = stored
      ? path.join(virtualRoot, createHash('sha256').update(id).digest('hex'))
      : path.resolve(base, entry.root)
    if ((!stored && within(virtualRoot, root)) || roots.has(root)) {
      throw loaderError('ERR_PNPM_LOADER_MANIFEST', `Conflicting package root: ${root}`)
    }
    roots.add(root)
    packages.set(id, { ...entry, id, root, stored, dependencies: new Map(Object.entries(entry.dependencies ?? {})) })
  }
  for (const pkg of packages.values()) {
    for (const [alias, target] of pkg.dependencies) {
      if (!packages.has(target)) {
        throw loaderError('ERR_PNPM_LOADER_MANIFEST', `Unknown dependency ${alias} -> ${target} in ${pkg.id}`)
      }
    }
  }
  return packages
}

function validatePackage (id, entry) {
  if (!isRecord(entry) || !isRecord(entry.dependencies ?? {}) ||
    (Object.hasOwn(entry, 'files') && !isRecord(entry.files)) ||
    (Object.hasOwn(entry, 'root') && typeof entry.root !== 'string')) {
    throw loaderError('ERR_PNPM_LOADER_MANIFEST', `Invalid package entry: ${id}`)
  }
}

function isRecord (value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function indexFiles (pkg, files, directories, storeDir) {
  if (!pkg.stored) return
  directories.add(pkg.root)
  if (!Object.hasOwn(pkg.files, 'package.json')) {
    files.set(path.join(pkg.root, 'package.json'), { source: Buffer.from('{}') })
  }
  for (const [name, hash] of Object.entries(pkg.files)) {
    if (!validFilename(name) || typeof hash !== 'string' || !/^[a-f0-9]{128}(?:-exec)?$/.test(hash)) {
      throw loaderError('ERR_PNPM_LOADER_MANIFEST', `Invalid store file ${pkg.id}/${name}`)
    }
    const filename = path.join(pkg.root, name)
    files.set(filename, { blob: path.join(storeDir, 'files', hash.slice(0, 2), hash.slice(2)), hash: hash.slice(0, 128) })
    for (let parent = path.dirname(filename); within(pkg.root, parent); parent = path.dirname(parent)) {
      directories.add(parent)
    }
  }
}

function validFilename (name) {
  return name.length > 0 && !name.includes('\\') && !name.includes('\0') && !name.includes(':') &&
    name.split('/').every(part => part !== '' && part !== '.' && part !== '..' && part !== 'node_modules')
}

export function within (root, filename) {
  return filename === root || filename.startsWith(`${root}${path.sep}`)
}

function virtualFilesystem ({ files, directories, virtualRoot }) {
  function statSync (filename) {
    const normalized = path.resolve(filename)
    if (!within(virtualRoot, normalized)) return fs.statSync(filename)
    if (!files.has(normalized) && !directories.has(normalized)) throw missing(filename)
    return {
      isFile: () => files.has(normalized),
      isDirectory: () => directories.has(normalized),
      isSymbolicLink: () => false,
    }
  }
  function readFileSync (filename, encoding) {
    const normalized = path.resolve(filename)
    if (!within(virtualRoot, normalized)) return fs.readFileSync(filename, encoding)
    const entry = files.get(normalized)
    if (!entry) throw missing(filename)
    if (entry.source) return encoding ? entry.source.toString(encoding) : entry.source
    const source = fs.readFileSync(entry.blob)
    if (createHash('sha512').update(source).digest('hex') !== entry.hash) {
      throw loaderError('ERR_PNPM_LOADER_INTEGRITY', `Store file failed integrity verification: ${entry.blob}`)
    }
    return encoding ? source.toString(encoding) : source
  }
  return { statSync, readFileSync }
}

function missing (filename) {
  return loaderError('ENOENT', `No file in store manifest: ${filename}`)
}
