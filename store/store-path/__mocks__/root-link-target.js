const MAPPINGS = {
  '/src/workspace/project/tmp': '/',
  '/mnt/project/tmp': '/mnt/project',
}

module.exports = async function (file) {
  return MAPPINGS[file]
}
