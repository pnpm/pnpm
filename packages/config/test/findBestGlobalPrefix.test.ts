import findBestGlobalPrefix from '../lib/findBestGlobalPrefix'

test('findBestGlobalPrefix()', () => {
  if (process.platform !== 'win32') {
    // skipping on non-windows
    return
  }

  const env = {
    APPDATA: 'C:\\Users\\Imre\\AppData\\Roaming',
    LOCALAPPDATA: 'C:\\Users\\Imre\\AppData\\Local',
  }

  expect(
    // keep npm global prefix if is inside AppData\Local
    findBestGlobalPrefix('C:\\Users\\Imre\\AppData\\Local\\nvs\\default', env)).toEqual(
    'C:\\Users\\Imre\\AppData\\Local\\nvs\\default'
  )

  expect(
    // keep npm global prefix if is inside AppData\Roaming
    findBestGlobalPrefix('C:\\Users\\Imre\\AppData\\Roaming\\nvs\\default', env)).toEqual(
    'C:\\Users\\Imre\\AppData\\Roaming\\nvs\\default'
  )

  expect(
    // prefer location in AppData\Roaming
    findBestGlobalPrefix('C:\\foo', env)).toEqual(
    'C:\\Users\\Imre\\AppData\\Roaming\\npm'
  )

  expect(
    findBestGlobalPrefix('C:\\foo', {})).toEqual(
    'C:\\foo'
  )
})
