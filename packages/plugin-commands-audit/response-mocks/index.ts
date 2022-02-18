import path from 'path'
import loadJsonFile from 'load-json-file'

// eslint-disable-next-line
const response1 = loadJsonFile.sync<any>(path.join(__dirname, 'response1.json'))
// eslint-disable-next-line
const response2 = loadJsonFile.sync<any>(path.join(__dirname, 'response2.json'))
// eslint-disable-next-line
const response3 = loadJsonFile.sync<any>(path.join(__dirname, 'response3.json'))

export { response1, response2, response3 }
