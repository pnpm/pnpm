#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { existsSync, lstatSync, readFileSync, readdirSync } from 'node:fs'
import { basename, isAbsolute, posix, relative, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const args = parseArgs(process.argv.slice(2))

if (args.projects) {
  verifyPackedProjects(args)
} else if (args.packageDir && args.tarball) {
  verifyPackedPackage(args.packageDir, args.tarball)
} else {
  usage('expected either --projects/--pack-json/--tarball-dir or --package-dir/--tarball')
}

function verifyPackedProjects ({ projects, packJson, tarballDir }) {
  if (!packJson) usage('missing --pack-json')
  if (!tarballDir) usage('missing --tarball-dir')

  const projectById = new Map()
  for (const project of readJson(projects)) {
    if (!project.name || !project.version || !project.path) continue
    projectById.set(`${project.name}@${project.version}`, project.path)
  }

  const packedPackages = normalizeArray(readJson(packJson))
  for (const packedPackage of packedPackages) {
    const projectDir = projectById.get(`${packedPackage.name}@${packedPackage.version}`)
    if (!projectDir) {
      throw new Error(`Cannot find workspace project for packed package ${packedPackage.name}@${packedPackage.version}`)
    }
    const tarball = isAbsolute(packedPackage.filename)
      ? packedPackage.filename
      : resolve(tarballDir, packedPackage.filename)
    verifyPackedPackage(projectDir, tarball)
  }
}

function verifyPackedPackage (packageDir, tarball) {
  const packageRoot = resolve(packageDir)
  const manifestPath = resolve(packageRoot, 'package.json')
  const manifest = readJson(manifestPath)
  const expectedFiles = collectExpectedFiles(packageRoot, manifest)
  if (expectedFiles.size === 0) {
    console.log(`No literal package payload files declared by ${manifest.name ?? packageDir}`)
    return
  }
  if (!existsSync(tarball)) {
    throw new Error(`Missing tarball ${tarball}`)
  }

  for (const file of [...expectedFiles].sort()) {
    const source = resolvePayloadPath(packageRoot, file)
    const stat = lstatSync(source, { throwIfNoEntry: false })
    if (stat?.isSymbolicLink()) {
      throw new Error(`Payload file cannot be a symlink: ${source}`)
    }
    if (!stat?.isFile()) {
      throw new Error(`Missing payload file ${source}`)
    }

    const packed = extractTarEntry(tarball, `package/${file}`)
    const sourceHash = sha256(readFileSync(source))
    const packedHash = sha256(packed)
    if (sourceHash !== packedHash) {
      throw new Error(`Packed payload differs from source for ${manifest.name ?? packageDir}: ${file}`)
    }
  }

  console.log(`Verified ${expectedFiles.size} payload files for ${manifest.name ?? packageDir}`)
}

function collectExpectedFiles (packageDir, manifest) {
  const files = new Set()
  const excluded = (manifest.files ?? [])
    .filter((file) => typeof file === 'string' && file.startsWith('!'))
    .map((file) => globMatcher(file.slice(1)))

  const add = (file) => {
    if (typeof file !== 'string' || file.length === 0) return
    if (file === 'package.json' || file === './package.json') return
    if (!file.startsWith('./') && file.startsWith('.') && !file.startsWith('..')) return
    const relative = normalizeRelative(file)
    if (relative === 'package.json') return
    if (isExcluded(relative, excluded)) return

    const source = resolvePayloadPath(packageDir, relative)
    const stat = lstatSync(source, { throwIfNoEntry: false })
    if (stat?.isSymbolicLink()) {
      throw new Error(`Payload file cannot be a symlink: ${source}`)
    }
    if (stat?.isFile()) {
      files.add(relative)
      return
    }
    if (!stat?.isDirectory()) {
      throw new Error(`Missing payload file ${source}`)
    }
    for (const entry of readdirSync(source, { withFileTypes: true })) {
      add(posix.join(relative, entry.name))
    }
  }

  for (const file of manifest.files ?? []) {
    if (typeof file === 'string' && !file.startsWith('!') && !/[?*[]/.test(file)) add(file)
  }
  add(manifest.main)
  add(manifest.module)
  add(manifest.types)
  add(manifest.typings)
  addExportTargets(manifest.exports, add)
  addExportTargets(manifest.browser, add)
  for (const file of Object.values(typeof manifest.bin === 'string' ? { default: manifest.bin } : manifest.bin ?? {})) add(file)
  for (const file of manifest.publishConfig?.executableFiles ?? []) add(file)

  return files
}

function addExportTargets (value, add) {
  if (typeof value === 'string') {
    if (value.startsWith('./')) add(value)
    return
  }
  if (Array.isArray(value)) {
    for (const item of value) addExportTargets(item, add)
    return
  }
  if (value && typeof value === 'object') {
    for (const item of Object.values(value)) addExportTargets(item, add)
  }
}

function extractTarEntry (tarball, entry) {
  const result = spawnSync('tar', ['-xOf', tarball, entry], {
    encoding: 'buffer',
    maxBuffer: 1024 * 1024 * 64,
  })
  const stderr = (result.stderr ?? Buffer.from('')).toString()
  if (result.error) {
    throw new Error(
      `Failed to extract packed entry ${entry} from ${tarball}: ${result.error.message}${stderr ? `\n${stderr}` : ''}`
    )
  }
  if (result.status !== 0) {
    throw new Error(`Missing packed entry ${entry} in ${tarball}${stderr ? `\n${stderr}` : ''}`)
  }
  return result.stdout
}

function globMatcher (pattern) {
  const normalized = normalizeRelative(pattern)
  const regexp = globToRegExp(normalized)
  const basenameRegexp = normalized.includes('/') ? null : globToRegExp(normalized)
  return (file) => regexp.test(file) || (basenameRegexp?.test(basename(file)) ?? false)
}

function isExcluded (file, excluded) {
  return excluded.some((matcher) => matcher(file))
}

function globToRegExp (pattern) {
  return new RegExp(
    '^' +
    pattern
      .replace(/[.$+^{}()|[\]\\]/g, '\\$&')
      .replace(/\*\*\//g, '\x00')
      .replace(/\*\*/g, '\x01')
      .replace(/\*/g, '[^/]*')
      .replace(/\?/g, '[^/]')
      .replace(/\x00/g, '(?:.*/)?')
      .replace(/\x01/g, '.*') +
    '$'
  )
}

function normalizeRelative (file) {
  const normalizedInput = file.replace(/\\/g, '/')
  if (normalizedInput.startsWith('/') || /^[A-Za-z]:\//.test(normalizedInput)) {
    throw new Error(`Payload file path must be relative: ${file}`)
  }
  const normalized = posix.normalize(normalizedInput.replace(/^\.\//, ''))
  if (normalized === '.' || normalized === '..' || normalized.startsWith('../') || normalized.split('/').includes('..')) {
    throw new Error(`Payload file path escapes package directory: ${file}`)
  }
  return normalized
}

function resolvePayloadPath (packageDir, file) {
  const source = resolve(packageDir, file)
  const relativeSource = relative(packageDir, source)
  if (relativeSource === '' || relativeSource.startsWith('..') || isAbsolute(relativeSource)) {
    throw new Error(`Payload file path escapes package directory: ${file}`)
  }
  return source
}

function sha256 (buffer) {
  return createHash('sha256').update(buffer).digest('hex')
}

function readJson (file) {
  return JSON.parse(readFileSync(file, 'utf8'))
}

function normalizeArray (value) {
  return Array.isArray(value) ? value : [value]
}

function parseArgs (rawArgs) {
  const args = {}
  for (let i = 0; i < rawArgs.length; i++) {
    const arg = rawArgs[i]
    if (arg === '--projects') {
      args.projects = rawArgs[++i]
    } else if (arg === '--pack-json') {
      args.packJson = rawArgs[++i]
    } else if (arg === '--tarball-dir') {
      args.tarballDir = rawArgs[++i]
    } else if (arg === '--package-dir') {
      args.packageDir = rawArgs[++i]
    } else if (arg === '--tarball') {
      args.tarball = rawArgs[++i]
    } else {
      usage(`unknown argument: ${arg}`)
    }
  }
  return args
}

function usage (message) {
  console.error(message)
  console.error(`Usage:
  verify-packed-package-payload.mjs --package-dir <dir> --tarball <file>
  verify-packed-package-payload.mjs --projects <projects.json> --pack-json <pack.json> --tarball-dir <dir>`)
  process.exit(1)
}
