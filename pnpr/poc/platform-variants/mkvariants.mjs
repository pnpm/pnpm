// Convert an npm package that uses per-platform optionalDependencies into a
// primary archive plus platform variant archives, for the pnpr platform
// variants PoC.
//
// usage: node mkvariants.mjs <name> <version> <adapter> <outDir> [--only tag,tag]
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'

const [name, version, adapterName, outDir, ...rest] = process.argv.slice(2)
const only = rest[0] === '--only' ? new Set(rest[1].split(',')) : null
const NPM = 'https://registry.npmjs.org/'
const work = path.join(outDir, 'work', name.replace('/', '__'))
fs.rmSync(work, { recursive: true, force: true })
fs.mkdirSync(work, { recursive: true })

async function manifest (pkg, ver) {
  const res = await fetch(NPM + pkg.replaceAll('/', '%2f') + '/' + ver)
  if (!res.ok) throw new Error(`${pkg}@${ver}: ${res.status}`)
  return res.json()
}

async function download (pkg, ver, dest) {
  const m = await manifest(pkg, ver)
  const res = await fetch(m.dist.tarball)
  const tgz = path.join(work, `${pkg.replace('/', '__')}.tgz`)
  fs.writeFileSync(tgz, Buffer.from(await res.arrayBuffer()))
  fs.mkdirSync(dest, { recursive: true })
  execFileSync('tar', ['xzf', tgz, '-C', dest, '--strip-components=1', '--no-same-owner'])
  return m
}

function readJson (file) { return JSON.parse(fs.readFileSync(file, 'utf8')) }
function writeJson (file, value) { fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n') }

function pack (dir, out) {
  const stage = fs.mkdtempSync(path.join(work, 'stage-'))
  fs.cpSync(dir, path.join(stage, 'package'), { recursive: true })
  execFileSync('tar', ['czf', out, '--owner=0', '--group=0', '--numeric-owner', '-C', stage, 'package'])
  fs.rmSync(stage, { recursive: true, force: true })
  const integrity = 'sha512-' + crypto.createHash('sha512').update(fs.readFileSync(out)).digest('base64')
  return integrity
}

function dropPlatformDeps (pj, platNames) {
  for (const field of ['optionalDependencies', 'dependencies']) {
    if (!pj[field]) continue
    for (const n of platNames) delete pj[field][n]
    if (Object.keys(pj[field]).length === 0) delete pj[field]
  }
}

function dropInstallScripts (pj) {
  if (!pj.scripts) return
  for (const s of ['preinstall', 'install', 'postinstall']) delete pj.scripts[s]
}

function copyNodeFiles (platDir, dest) {
  for (const f of fs.readdirSync(platDir)) {
    if (f.endsWith('.node')) fs.copyFileSync(path.join(platDir, f), path.join(dest, f))
  }
}

const adapters = {
  // Go binary, statically linked on Linux.
  esbuild: {
    staticLinux: true,
    variant (dir, platDir, pj) {
      const exe = fs.existsSync(path.join(platDir, 'esbuild.exe')) ? 'esbuild.exe' : 'bin/esbuild'
      const target = exe === 'esbuild.exe' ? 'bin/esbuild.exe' : 'bin/esbuild'
      fs.copyFileSync(path.join(platDir, exe), path.join(dir, target))
      fs.chmodSync(path.join(dir, target), 0o755)
      if (target !== 'bin/esbuild') fs.rmSync(path.join(dir, 'bin/esbuild'))
      pj.bin = { esbuild: target }
      const main = path.join(dir, 'lib/main.js')
      const src = fs.readFileSync(main, 'utf8').replace(
        'var ESBUILD_BINARY_PATH = process.env.ESBUILD_BINARY_PATH || ESBUILD_BINARY_PATH;',
        `var ESBUILD_BINARY_PATH = process.env.ESBUILD_BINARY_PATH || require("path").join(__dirname, "..", ${JSON.stringify(target)});`
      )
      fs.writeFileSync(main, src)
    }
  },
  // Rust CLI binary behind a JavaScript shim.
  biome: {
    variant (dir, platDir, pj) {
      const exe = fs.existsSync(path.join(platDir, 'biome.exe')) ? 'biome.exe' : 'biome'
      const target = `bin/${exe}`
      fs.rmSync(path.join(dir, 'bin/biome'))
      fs.copyFileSync(path.join(platDir, exe), path.join(dir, target))
      fs.chmodSync(path.join(dir, target), 0o755)
      pj.bin = { biome: target }
    }
  },
  // napi-rs addon: the generated loader tries a local `.node` file first.
  napi: {
    variant (dir, platDir) { copyNodeFiles(platDir, dir) }
  },
  // napi-rs addon whose primary is the wasm32-wasi build, with its wasm
  // runtime bundled so that only hosts that link the primary get it.
  'napi-wasm-primary': {
    variant (dir, platDir) { copyNodeFiles(platDir, dir) },
    async primary (dir, pj, platforms) {
      const wasm = platforms.find(p => p.manifest.cpu?.[0] === 'wasm32')
      if (!wasm) return
      const wasmDir = path.join(work, 'plat', 'wasm')
      await download(wasm.name, wasm.manifest.version, wasmDir)
      for (const f of fs.readdirSync(wasmDir)) {
        if (f === 'package.json' || f === 'README.md') continue
        fs.cpSync(path.join(wasmDir, f), path.join(dir, f), { recursive: true })
      }
      const bundled = wasm.manifest.bundleDependencies ?? wasm.manifest.bundledDependencies ?? []
      pj.dependencies = { ...(pj.dependencies ?? {}) }
      for (const dep of bundled) pj.dependencies[dep] = wasm.manifest.dependencies[dep]
      pj.bundleDependencies = bundled
    }
  },
  // rolldown's loader in dist/shared loads `../rolldown-binding.<triple>.node`.
  rolldown: {
    variant (dir, platDir) { copyNodeFiles(platDir, path.join(dir, 'dist')) }
  },
  // lightningcss loads `../lightningcss.<triple>.node` from node/index.js.
  lightningcss: {
    variant (dir, platDir) { copyNodeFiles(platDir, dir) }
  },
}

const adapter = adapters[adapterName]
const mainDir = path.join(work, 'main')
const mainPj = await download(name, version, mainDir)
const optional = Object.keys(mainPj.optionalDependencies ?? {})
const platforms = []
for (const dep of optional) {
  const m = await manifest(dep, mainPj.optionalDependencies[dep])
  if (!m.os && !m.cpu) continue
  platforms.push({ name: dep, manifest: m })
}
const platNames = platforms.map(p => p.name)

// Primary: the package without its platform packages or install scripts.
const primaryDir = path.join(work, 'primary')
fs.cpSync(mainDir, primaryDir, { recursive: true })
const primaryPj = readJson(path.join(primaryDir, 'package.json'))
dropPlatformDeps(primaryPj, platNames)
dropInstallScripts(primaryPj)
writeJson(path.join(primaryDir, 'package.json'), primaryPj)

fs.mkdirSync(outDir, { recursive: true })
const base = name.replace('@', '').replace('/', '-')
const result = { name, version, primary: { file: path.join(outDir, `${base}-${version}.tgz`) }, variants: [] }

for (const plat of platforms) {
  const m = plat.manifest
  const cpu = m.cpu?.[0]
  if (!cpu || cpu === 'wasm32') continue
  let tag = plat.name.split('/').pop().replace(/^(cli|oxide|binding)-/, '')
  if (name === 'lightningcss') tag = tag.replace(/^lightningcss-/, '')
  if (only && !only.has(tag)) continue
  const platDir = path.join(work, 'plat', plat.name.replace('/', '__'))
  await download(plat.name, m.version, platDir)
  const dir = path.join(work, 'variant', tag)
  fs.cpSync(primaryDir, dir, { recursive: true })
  const pj = readJson(path.join(dir, 'package.json'))
  adapter.variant(dir, platDir, pj)
  // The archive declares where it runs, as platform tags; glibc is written
  // as no part.
  const platformTags = []
  for (const os of m.os) {
    if (os === 'linux') {
      const libcs = m.libc ?? (adapter.staticLinux ? ['glibc', 'musl'] : ['glibc'])
      for (const libc of libcs) platformTags.push(libc === 'musl' ? `${os}-${cpu}-musl` : `${os}-${cpu}`)
    } else {
      platformTags.push(`${os}-${cpu}`)
    }
  }
  for (const field of ['os', 'cpu', 'libc']) delete pj[field]
  pj.variant = { platforms: platformTags }
  writeJson(path.join(dir, 'package.json'), pj)
  const file = path.join(outDir, `${base}-${version}~${tag}.tgz`)
  const integrity = pack(dir, file)
  result.variants.push({ label: tag, platforms: platformTags, file, integrity })
}
if (adapter.primary) {
  const pj = readJson(path.join(primaryDir, 'package.json'))
  await adapter.primary(primaryDir, pj, platforms)
  writeJson(path.join(primaryDir, 'package.json'), pj)
}
result.primary.integrity = pack(primaryDir, result.primary.file)
writeJson(path.join(outDir, `${base}-${version}.variants.json`), result)
console.log(`${name}@${version}: primary + ${result.variants.length} variants -> ${outDir}`)
