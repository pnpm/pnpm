import fs from 'node:fs'
import module from 'node:module'
import path from 'node:path'
import process from 'node:process'
import readline from 'node:readline'
import { fileURLToPath, pathToFileURL } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(__dirname, '../../../../..')
const packlistPkgDir = path.join(repoRoot, 'pnpm11/fs/packlist')
const requireFromPacklist = module.createRequire(path.join(packlistPkgDir, 'package.json'))
const npmPacklist = requireFromPacklist('npm-packlist')

function findWorkspaceTsEntry (specifier, parentURL) {
  let dir = parentURL?.startsWith('file:')
    ? path.dirname(fileURLToPath(parentURL))
    : packlistPkgDir
  while (true) {
    const candidate = path.join(dir, 'node_modules', specifier, 'src/index.ts')
    if (fs.existsSync(candidate)) {
      return fs.realpathSync(candidate)
    }
    const parent = path.dirname(dir)
    if (parent === dir) return null
    dir = parent
  }
}

async function loadPnpmPacklist () {
  module.registerHooks({
    resolve (specifier, context, nextResolve) {
      if (specifier.startsWith('.') && specifier.endsWith('.js') && context.parentURL?.startsWith('file:')) {
        const candidate = path.resolve(path.dirname(fileURLToPath(context.parentURL)), specifier.slice(0, -3) + '.ts')
        if (fs.existsSync(candidate)) {
          return { url: pathToFileURL(fs.realpathSync(candidate)).href, shortCircuit: true }
        }
      }
      if (specifier.startsWith('@pnpm/')) {
        const candidate = findWorkspaceTsEntry(specifier, context.parentURL)
        if (candidate != null) {
          return { url: pathToFileURL(candidate).href, shortCircuit: true }
        }
      }
      return nextResolve(specifier, context)
    },
  })
  const mod = await import(pathToFileURL(path.join(packlistPkgDir, 'src/index.ts')).href)
  return mod.packlist
}

const packlist = await loadPnpmPacklist()
const normalize = (list) => list.map((entry) => entry.replaceAll('\\', '/').replace(/^\.\//, '')).sort()

function normalizeNpmTreePackage (manifest) {
  const stripDotSlash = (p) => p.replace(/^\.[/\\]/, '')
  const normalized = { ...manifest }
  if (typeof normalized.main === 'string') {
    normalized.main = stripDotSlash(normalized.main)
  }
  if (typeof normalized.browser === 'string') {
    normalized.browser = stripDotSlash(normalized.browser)
  }
  if (typeof normalized.bin === 'string') {
    normalized.bin = { [normalized.name ?? 'pkg']: stripDotSlash(normalized.bin) }
  } else if (normalized.bin != null && typeof normalized.bin === 'object') {
    const bin = {}
    for (const [k, v] of Object.entries(normalized.bin)) {
      if (typeof v === 'string') bin[k] = stripDotSlash(v)
    }
    normalized.bin = bin
  }
  return normalized
}

if (process.argv.includes('--stdio')) {
  const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity })
  for await (const line of rl) {
    if (!line.trim()) continue
    const { root, manifest } = JSON.parse(line)
    const treeNode = {
      path: root,
      package: normalizeNpmTreePackage(manifest),
      isProjectRoot: true,
      isLink: false,
      edgesOut: new Map(),
    }
    treeNode.target = treeNode
    const [npm, pnpm] = await Promise.all([
      npmPacklist(treeNode),
      packlist(root, { manifest }),
    ])
    process.stdout.write(`${JSON.stringify({ npm: normalize(npm), pnpm: normalize(pnpm) })}\n`)
  }
} else {
  process.stdin.setEncoding('utf8')
  let input = ''
  for await (const chunk of process.stdin) input += chunk
  const results = []
  for (const { root, manifest } of JSON.parse(input)) {
    results.push(normalize(await packlist(root, { manifest })))
  }
  process.stdout.write(JSON.stringify(results))
}
