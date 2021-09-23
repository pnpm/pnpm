import * as execa from 'execa'
import path from 'path'

function build (target: string) {
  execa.sync('pkg', ['../pnpm/dist/pnpm.cjs', `--out-path=../artifacts/${target}`, `--targets=node14-${target}`], {
    cwd: path.join(__dirname, '..'),
    stdio: 'inherit',
  })
}

build('win-x64')
build('linux-x64')
build('linuxstatic-x64')
build('macos-x64')

const isM1Mac = process.platform === 'darwin' && process.arch === 'arm64'
if (process.platform === 'linux' || isM1Mac) {
  build('macos-arm64')
  build('linux-arm64')
}
