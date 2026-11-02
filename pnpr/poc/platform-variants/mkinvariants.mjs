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

const linuxX64 = ['linux-x64']
const darwin = ['darwin-arm64']
// Each case is accepted (`ok`, `deps-differ`, `bundled-extra-dep`) or
// rejected with the reason its name gives.
const cases = {
  ok: { variants: [{ label: 'linux', platforms: linuxX64 }, { label: 'darwin', platforms: darwin }] },
  'name-differs': { variants: [{ label: 'linux', platforms: linuxX64, pj: { name: '@variants-poc/other' } }] },
  'deps-differ': { variants: [{ label: 'linux', platforms: linuxX64, pj: { dependencies: { 'is-number': '7.0.0' } } }] },
  'peers-differ': { variants: [{ label: 'linux', platforms: linuxX64, pj: { peerDependencies: { react: '*' } } }] },
  'requires-build': { variants: [{ label: 'linux', platforms: linuxX64, pj: { scripts: { postinstall: 'node x.js' } } }] },
  'platform-overlap': { variants: [{ label: 'linux', platforms: linuxX64 }, { label: 'static', platforms: ['linux-x64', 'linux-x64-musl'] }] },
  'os-field': { variants: [{ label: 'linux', platforms: linuxX64, pj: { os: ['linux'] } }] },
  'non-canonical-tag': { variants: [{ label: 'linux', platforms: ['linux-x64-glibc'] }] },
  'no-variant-field': { variants: [{ label: 'linux', platforms: null }] },
  'bundled-extra-dep': {
    variants: [{
      label: 'linux',
      platforms: linuxX64,
      pj: { dependencies: { 'is-number': '7.0.0' }, bundleDependencies: ['is-number'] },
    }],
  },
  'undeclared-attachment': { variants: [{ label: 'linux', platforms: linuxX64, undeclared: true }] },
  'declared-not-attached': { variants: [{ label: 'linux', platforms: linuxX64, missing: true }] },
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
    const file = path.join(outDir, `${base}-${version}~${v.label}.tgz`)
    const pj = { ...primaryPj, ...(v.platforms && { variant: { platforms: v.platforms } }), ...(v.pj ?? {}) }
    const integrity = pack(pj, { 'index.js': `module.exports = ${JSON.stringify(v.label)}\n` }, file)
    result.variants.push({ label: v.label, file, integrity, undeclared: v.undeclared, missing: v.missing })
  }
  fs.writeFileSync(path.join(outDir, `${base}.variants.json`), JSON.stringify(result, null, 2))
}
console.log(Object.keys(cases).join(' '))
