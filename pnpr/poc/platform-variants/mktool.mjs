// Repackage a GitHub-released executable as a pnpr package with platform
// variants and a stub primary.
// usage: node mktool.mjs <outDir> <name> <version> <bin> <assetsJson>
// assetsJson: [{ label, url, platforms, exe }] where platforms are platform
// tags and exe is the executable's path inside the downloaded archive.
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'

const [outDirArg, name, version, bin, assetsFile] = process.argv.slice(2)
const outDir = path.resolve(outDirArg)
const assets = JSON.parse(fs.readFileSync(assetsFile, 'utf8'))
const base = name.split('/').pop()
const work = path.join(outDir, 'work', base)
fs.rmSync(work, { recursive: true, force: true })
fs.mkdirSync(work, { recursive: true })

function pack (dir, out) {
  execFileSync('tar', ['czf', out, '--owner=0', '--group=0', '-C', dir, 'package'])
  return 'sha512-' + crypto.createHash('sha512').update(fs.readFileSync(out)).digest('base64')
}

const stub = path.join(work, 'primary')
fs.mkdirSync(path.join(stub, 'package/bin'), { recursive: true })
fs.writeFileSync(path.join(stub, 'package/package.json'), JSON.stringify({ name, version, bin: { [bin]: `bin/${bin}.js` } }, null, 2))
fs.writeFileSync(path.join(stub, `package/bin/${bin}.js`),
  `#!/usr/bin/env node\nconsole.error(${JSON.stringify(`${name}@${version} has no build for`)}, process.platform, process.arch)\nprocess.exit(1)\n`)
const result = { name, version, primary: { file: path.join(outDir, `${base}-${version}.tgz`) }, variants: [] }
result.primary.integrity = pack(stub, result.primary.file)

for (const asset of assets) {
  const dl = path.join(work, 'dl', asset.label)
  fs.mkdirSync(dl, { recursive: true })
  const archive = path.join(dl, path.basename(new URL(asset.url).pathname))
  const res = await fetch(asset.url)
  if (!res.ok) throw new Error(`${asset.url}: ${res.status}`)
  fs.writeFileSync(archive, Buffer.from(await res.arrayBuffer()))
  if (archive.endsWith('.zip')) execFileSync('unzip', ['-q', archive, '-d', dl])
  else execFileSync('tar', ['xf', archive, '-C', dl])
  const exeName = asset.exe.endsWith('.exe') ? `${bin}.exe` : bin
  const pkg = path.join(work, 'variant', asset.label)
  fs.mkdirSync(path.join(pkg, 'package/bin'), { recursive: true })
  fs.copyFileSync(path.join(dl, asset.exe), path.join(pkg, 'package/bin', exeName))
  fs.chmodSync(path.join(pkg, 'package/bin', exeName), 0o755)
  fs.writeFileSync(path.join(pkg, 'package/package.json'), JSON.stringify({ name, version, bin: { [bin]: `bin/${exeName}` }, variant: { platforms: asset.platforms } }, null, 2))
  const file = path.join(outDir, `${base}-${version}~${asset.label}.tgz`)
  result.variants.push({ label: asset.label, file, integrity: pack(pkg, file) })
}
fs.writeFileSync(path.join(outDir, `${base}-${version}.variants.json`), JSON.stringify(result, null, 2))
console.log(`${name}@${version}: ${result.variants.length} variants`)
