const path = require('path')
const fs = require('fs')

const platform = process.platform == 'win32'
  ? 'win'
  : process.platform == 'darwin'
  ? 'macos'
  : process.platform
const arch = platform == 'win' && process.arch == 'ia32' ? 'x86' : process.arch

const pkgName = `@pnpm/${platform}-${arch}`
const pkgJson = require.resolve(`${pkgName}/package.json`)
const subpkg = JSON.parse(fs.readFileSync(pkgJson, 'utf8'))

if (subpkg.bin != null) {
  const executable = subpkg.bin.pnpm
  const bin = path.resolve(path.dirname(pkgJson), executable)

  linkSync(bin, path.resolve(process.cwd(), executable))

  if (platform == 'win') {
    const pkg = JSON.parse(fs.readFileSync(path.resolve(process.cwd(), 'package.json')))
    fs.writeFileSync(path.resolve(process.cwd(), 'pnpm'), 'This file intentionally left blank')
    pkg.bin.pnpm = 'pnpm.exe'
    fs.writeFileSync(path.resolve(process.cwd(), 'package.json'), JSON.stringify(pkg, null, 2))
  }
}

function linkSync(src, dest) {
  try {
    fs.unlinkSync(dest)
  } catch (e) {
    if (e.code != 'ENOENT') {
      throw e
    }
  }
  return fs.linkSync(src, dest)
}
