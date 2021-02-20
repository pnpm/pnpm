module.exports = async function requireOrImportPnpmfile (pnpmfilePath) {
  try {
    return require(pnpmfilePath)
  } catch (err) {
    return (await import(pnpmfilePath)).default
  }
}
