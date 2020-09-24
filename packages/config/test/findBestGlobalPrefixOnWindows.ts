import findBestGlobalPrefixOnWindows from '../src/findBestGlobalPrefixOnWindows'

test('findBestGlobalPrefixOnWindows()', () => {
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
    findBestGlobalPrefixOnWindows('C:\\Users\\Imre\\AppData\\Local\\nvs\\default', env)).toEqual(
    'C:\\Users\\Imre\\AppData\\Local\\nvs\\default'
  )

  expect(
    // keep npm global prefix if is inside AppData\Roaming
    findBestGlobalPrefixOnWindows('C:\\Users\\Imre\\AppData\\Roaming\\nvs\\default', env)).toEqual(
    'C:\\Users\\Imre\\AppData\\Roaming\\nvs\\default'
  )

  expect(
    // prefer location in AppData\Roaming
    findBestGlobalPrefixOnWindows('C:\\foo', env)).toEqual(
    'C:\\Users\\Imre\\AppData\\Roaming\\npm'
  )

  expect(
    findBestGlobalPrefixOnWindows('C:\\foo', {})).toEqual(
    'C:\\foo'
  )
})
