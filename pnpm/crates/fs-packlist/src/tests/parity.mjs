import fs from 'node:fs'
import module from 'node:module'
import path from 'node:path'
import process from 'node:process'
import readline from 'node:readline'
import { fileURLToPath, pathToFileURL } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const oraclesDir = path.resolve(__dirname, '../../../../test-oracles')
const requireFromOracles = module.createRequire(path.join(oraclesDir, 'package.json'))
const npmPacklist = requireFromOracles('npm-packlist')
const { packlist } = await import(pathToFileURL(requireFromOracles.resolve('@pnpm/fs.packlist')).href)
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
    const pnpmNormalized = normalize(pnpm)
    const pnpmLinks = pnpmNormalized.filter((rel) => {
      try {
        return fs.lstatSync(path.join(root, rel)).isSymbolicLink()
      } catch {
        return false
      }
    })
    process.stdout.write(`${JSON.stringify({ npm: normalize(npm), pnpm: pnpmNormalized, pnpmLinks })}\n`)
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
