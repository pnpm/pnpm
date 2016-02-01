var pnpm = {}

Object.defineProperty(pnpm, 'install', {
  enumerable: true,
  get: function () {
    return require('./lib/cmd/install')
  }
})

module.exports = pnpm
