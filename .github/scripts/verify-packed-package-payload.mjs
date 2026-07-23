#!/usr/bin/env node
// Verifies that packed tarballs contain every payload file the manifest
// declares (`files`, `main`, `module`, `types`, `exports`, `browser`,
// `bin`, `publishConfig.executableFiles`), so a packing regression fails
// the release before the first immutable npm publish
// (https://github.com/pnpm/pnpm/issues/13164).
//
// Release mode consumes the output of `pnpm pack --dry-run --json`
// (an entry per project with the tarball's file list), so no tarball is
// ever written or read. The single-package mode reads one real tarball's
// listing for debugging, e.g. against a downloaded npm artifact.
import { lstatSync, readFileSync, readdirSync } from 'node:fs'
import { basename, isAbsolute, posix, relative, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const args = parseArgs(process.argv.slice(2))

if (args.projects) {
  verifyPackedProjects(args)
} else if (args.packageDir && args.tarball) {
  verifyPackage(args.packageDir, readTarballListing(args.tarball), args.tarball)
} else {
  usage('expected either --projects/--pack-json or --package-dir/--tarball')
}

function verifyPackedProjects ({ projects, packJsons }) {
  if (packJsons.length === 0) usage('missing --pack-json')

  const packedById = new Map()
  for (const packJson of packJsons) {
    for (const packedPackage of normalizeArray(readJson(packJson))) {
      if (!Array.isArray(packedPackage.files)) {
        throw new Error(`Packed entry for ${packedPackage.name}@${packedPackage.version} carries no file list; pass the output of \`pnpm pack --dry-run --json\``)
      }
      packedById.set(
        `${packedPackage.name}@${packedPackage.version}`,
        new Set(packedPackage.files.map((file) => file.path))
      )
    }
  }

  const publishable = readJson(projects).filter(
    (project) => !project.private && project.name && project.version && project.path
  )
  if (publishable.length === 0) {
    throw new Error(`No publishable projects listed in ${projects}`)
  }

  const failures = []
  const unpacked = publishable.filter((project) => !packedById.has(`${project.name}@${project.version}`))
  if (unpacked.length > 0) {
    failures.push(`Publishable projects missing from the pack result: ${unpacked.map((project) => project.name).join(', ')}`)
  }

  for (const project of publishable) {
    const packedFiles = packedById.get(`${project.name}@${project.version}`)
    if (!packedFiles) continue
    try {
      verifyPackage(project.path, packedFiles, 'pack result')
    } catch (error) {
      failures.push(error.message)
    }
  }
  if (failures.length > 0) {
    throw new Error(`Payload verification failed:\n\n${failures.join('\n\n')}`)
  }
  console.log(`Verified payloads of ${publishable.length} packages`)
}

function verifyPackage (packageDir, packedFiles, packedSource) {
  const packageRoot = resolve(packageDir)
  const manifest = readJson(resolve(packageRoot, 'package.json'))
  const expectedFiles = collectExpectedFiles(packageRoot, manifest)
  if (expectedFiles.size === 0) {
    console.log(`No literal package payload files declared by ${manifest.name ?? packageDir}`)
    return
  }

  const missing = [...expectedFiles].filter((file) => !packedFiles.has(file)).sort()
  if (missing.length > 0) {
    throw new Error(`Payload files of ${manifest.name ?? packageDir} missing from ${packedSource}:\n  ${missing.join('\n  ')}`)
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

function readTarballListing (tarball) {
  const result = spawnSync('tar', ['-tf', tarball], {
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 64,
  })
  if (result.error) {
    throw new Error(`Failed to list ${tarball}: ${result.error.message}`)
  }
  if (result.status !== 0) {
    throw new Error(`Failed to list ${tarball}${result.stderr ? `\n${result.stderr}` : ''}`)
  }
  return new Set(
    result.stdout
      .split('\n')
      .filter((entry) => entry.startsWith('package/'))
      .map((entry) => entry.slice('package/'.length))
  )
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

function readJson (file) {
  return JSON.parse(readFileSync(file, 'utf8'))
}

function normalizeArray (value) {
  return Array.isArray(value) ? value : [value]
}

function parseArgs (rawArgs) {
  const args = { packJsons: [] }
  for (let i = 0; i < rawArgs.length; i++) {
    const arg = rawArgs[i]
    if (arg === '--projects') {
      args.projects = rawArgs[++i]
    } else if (arg === '--pack-json') {
      args.packJsons.push(rawArgs[++i])
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
  verify-packed-package-payload.mjs --projects <projects.json> --pack-json <pack.json> [--pack-json <more.json>...]
  verify-packed-package-payload.mjs --package-dir <dir> --tarball <file>`)
  process.exit(1)
}
