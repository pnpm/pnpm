import diable = require('diable')
import path = require('path')

const pnpm = path.join(__dirname, 'bin', 'pnpm.js')

export default () => diable.daemonize(pnpm, ['server', 'start'], {stdio: 'inherit'})
