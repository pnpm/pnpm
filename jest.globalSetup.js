const { start, prepare } = require('@pnpm/registry-mock')

module.exports = () => {
  prepare()
  global.__SERVER__ = start()
}
