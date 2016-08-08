var pnpm = {}

Object.defineProperty(pnpm, 'install', {
  enumerable: true,
  get: function () {
    return require('./lib/cmd/install')
  }
})

Object.defineProperty(pnpm, 'uninstall', {
  enumerable: true,
  get: function () {
    return require('./lib/cmd/uninstall')
  }
})

module.exports = pnpm
