module.exports = function err (error) {
  console.error('')
  console.error('Error: ' + (error.stack || error.message || error))
  process.exit(1)
}
