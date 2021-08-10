import execa from 'execa'
import path from 'path'

function build (target: string) {
  execa.sync('pkg', ['./dist/pnpm.cjs', `--out-path=../artifacts/${target}`, `--targets=node14-${target}`], {
    cwd: path.join(__dirname, '..'),
    stdio: 'inherit',
  })
}

build('win-x64')
build('linux-x64')
build('linuxstatic-x64')
build('macos-x64')
if (process.platform === 'linux' || process.platform === 'darwin') {
  build('macos-arm64')
}
