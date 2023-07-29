const value = process.env['npm_config_frozen_lockfile']

switch(value) {
  case undefined:
    process.stdout.write('unset')
    break
  case '':
    process.stdout.write('empty string')
    break
  default:
    process.stdout.write('string: ' + value)
    break
}
