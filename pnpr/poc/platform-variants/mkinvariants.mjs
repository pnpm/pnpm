// Build small packages that each break one publish invariant.
// usage: node mkinvariants.mjs <outDir>
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import crypto from 'node:crypto'

const outDir = path.resolve(process.argv[2])
fs.mkdirSync(outDir, { recursive: true })

function pack (pj, files, out) {
  const stage = fs.mkdtempSync(path.join(outDir, 'stage-'))
  fs.mkdirSync(path.join(stage, 'package'))
  fs.writeFileSync(path.join(stage, 'package/package.json'), JSON.stringify(pj, null, 2))
  for (const [name, content] of Object.entries(files)) fs.writeFileSync(path.join(stage, 'package', name), content)
  execFileSync('tar', ['czf', out, '--owner=0', '--group=0', '-C', stage, 'package'])
  fs.rmSync(stage, { recursive: true })
  return 'sha512-' + crypto.createHash('sha512').update(fs.readFileSync(out)).digest('base64')
}

const linuxX64 = [{ os: 'linux', cpu: 'x64', libc: 'glibc' }]
const darwin = [{ os: 'darwin', cpu: 'arm64' }]
const cases = {
  ok: { variants: [{ tag: 'linux-x64', platforms: linuxX64 }, { tag: 'darwin-arm64', platforms: darwin }] },
  'name-differs': { variants: [{ tag: 'linux-x64', platforms: linuxX64, pj: { name: '@variants-poc/other' } }] },
  'deps-differ': { variants: [{ tag: 'linux-x64', platforms: linuxX64, pj: { dependencies: { 'is-number': '7.0.0' } } }] },
  'requires-build': { variants: [{ tag: 'linux-x64', platforms: linuxX64, pj: { scripts: { postinstall: 'node x.js' } } }] },
  'platform-overlap': { variants: [{ tag: 'linux-x64', platforms: linuxX64 }, { tag: 'linux-x64-static', platforms: [{ os: 'linux', cpu: 'x64' }] }] },
  'os-field-excludes': { variants: [{ tag: 'linux-x64', platforms: linuxX64, pj: { os: ['darwin'] } }] },
  'bundled-extra-dep': {
    variants: [{
      tag: 'linux-x64',
      platforms: linuxX64,
      pj: { dependencies: { 'is-number': '7.0.0' }, bundleDependencies: ['is-number'] },
    }],
  },
  'undeclared-attachment': { variants: [{ tag: 'linux-x64', platforms: linuxX64, undeclared: true }] },
  'declared-not-attached': { variants: [{ tag: 'linux-x64', platforms: linuxX64, missing: true }] },
}

for (const [caseName, spec] of Object.entries(cases)) {
  const name = `@variants-poc/inv-${caseName}`
  const version = '1.0.0'
  const base = `inv-${caseName}`
  const primaryPj = { name, version, main: 'index.js' }
  const primary = path.join(outDir, `${base}-${version}.tgz`)
  const result = {
    name, version,
    primary: { file: primary, integrity: pack(primaryPj, { 'index.js': 'module.exports = "primary"\n' }, primary) },
    variants: [],
  }
  for (const v of spec.variants) {
    const file = path.join(outDir, `${base}-${version}~${v.tag}.tgz`)
    const integrity = pack({ ...primaryPj, ...(v.pj ?? {}) }, { 'index.js': `module.exports = ${JSON.stringify(v.tag)}\n` }, file)
    result.variants.push({ tag: v.tag, platforms: v.platforms, file, integrity, undeclared: v.undeclared, missing: v.missing })
  }
  fs.writeFileSync(path.join(outDir, `${base}.variants.json`), JSON.stringify(result, null, 2))
}
console.log(Object.keys(cases).join(' '))
