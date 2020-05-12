import test = require('tape')
import findBestGlobalPrefixOnWindows from '../src/findBestGlobalPrefixOnWindows'

test('findBestGlobalPrefixOnWindows()', t => {
  if (process.platform !== 'win32') {
    t.comment('skipping on non-windows')
    t.end()
    return
  }

  const env = {
    APPDATA: 'C:\\Users\\Imre\\AppData\\Roaming',
    LOCALAPPDATA: 'C:\\Users\\Imre\\AppData\\Local',
  }

  t.equal(
    findBestGlobalPrefixOnWindows('C:\\Users\\Imre\\AppData\\Local\\nvs\\default', env),
    'C:\\Users\\Imre\\AppData\\Local\\nvs\\default',
    'keep npm global prefix if is inside AppData\\Local'
  )

  t.equal(
    findBestGlobalPrefixOnWindows('C:\\Users\\Imre\\AppData\\Roaming\\nvs\\default', env),
    'C:\\Users\\Imre\\AppData\\Roaming\\nvs\\default',
    'keep npm global prefix if is inside AppData\\Roaming'
  )

  t.equal(
    findBestGlobalPrefixOnWindows('C:\\foo', env),
    'C:\\Users\\Imre\\AppData\\Roaming\\npm',
    'prefer location in AppData\\Roaming'
  )

  t.equal(
    findBestGlobalPrefixOnWindows('C:\\foo', {}),
    'C:\\foo'
  )

  t.end()
})
