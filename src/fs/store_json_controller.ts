import path = require('path')
import fs = require('fs')

export default function storeJsonController (storePath) {
  const storeJsonPath = path.join(storePath, 'store.json')

  return {
    read () {
      try {
        return JSON.parse(fs.readFileSync(storeJsonPath, 'utf8'))
      } catch (err) {
        return null
      }
    },
    save (storeJson) {
      fs.writeFileSync(storeJsonPath, JSON.stringify(storeJson, null, 2), 'utf8')
    }
  }
}
