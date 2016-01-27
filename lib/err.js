module.exports = function err (error) {
  console.error(err.stack)
  process.exit(1)
}
