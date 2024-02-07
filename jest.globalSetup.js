const { start, prepare } = require('@pnpm/registry-mock')

module.exports = () => {
  prepare()
  // TODO: FAIL TO START IF VERDACCIO IS ALREADY RUNNING ON THE SPECIFIED PORT!!!
  global.__SERVER__ = start({
    stdio: 'ignore',
  })
}
