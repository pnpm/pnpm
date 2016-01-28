module.exports = function err (error) {
  console.error('')
  if (error.host && error.path) {
    console.error('' + error.message)
    console.error('' + error.method + ' ' + error.host + error.path)
  } else {
    console.error('Error: ' + (error.stack || error.message || error))
  }
  process.exit(1)
}
