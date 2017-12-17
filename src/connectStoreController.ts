import {
  PackageResponse,
  RequestPackageFunction,
  RequestPackageOptions,
  Resolution,
  WantedDependency,
} from '@pnpm/package-requester'
import JsonSocket = require('json-socket')
import net = require('net')
import {StoreController} from 'package-store'
import uuid = require('uuid')

export default function (
  initOpts: object,
): Promise<StoreController> {
  const socket = new JsonSocket(new net.Socket());
  socket.connect(initOpts as any) // tslint:disable-line

  return new Promise((resolve, reject) => {
    socket.on('connect', () => {
      const waiters = createWaiters()

      socket.on('message', (message) => {
        if (message.err) {
          waiters.reject(message.action, message.err)
        } else {
          waiters.resolve(message.action, message.body)
        }
      })

      resolve({
        close: async () => {
          socket.end()
        },
        prune: async () => {
          socket.sendMessage({
            action: 'prune',
          }, (err) => err && console.error(err))
        },
        requestPackage: requestPackage.bind(null, socket, waiters),
        saveState: async () => {
          socket.sendMessage({
            action: 'saveState',
          }, (err) => err && console.error(err))
        },
        updateConnections: async (prefix: string, opts: {addDependencies: string[], removeDependencies: string[], prune: boolean}) => {
          socket.sendMessage({
            action: 'updateConnections',
            args: [prefix, opts],
          }, (err) => err && console.error(err))
        },
      })
    })
  })
}

function createWaiters () {
  const waiters = {}
  return {
    add (id: string) {
      waiters[id] = deffered()
      return waiters[id].promise
    },
    resolve (id: string, obj: object) {
      if (waiters[id]) {
        waiters[id].resolve(obj)
        delete waiters[id]
      }
    },
    reject (id: string, err: object) {
      if (waiters[id]) {
        waiters[id].reject(err)
        delete waiters[id]
      }
    },
  }
}

// tslint:disable-next-line
function noop () {}

function deffered<T> (): {
  promise: Promise<T>,
  resolve: (v: T) => void,
  reject: (err: Error) => void,
} {
  let pResolve: (v: T) => void = noop
  let pReject: (err: Error) => void = noop
  const promise = new Promise<T>((resolve, reject) => {
    pResolve = resolve
    pReject = reject
  })
  return {
    promise,
    reject: pReject,
    resolve: pResolve,
  }
}

function requestPackage (
  socket: JsonSocket,
  waiters: object,
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
): Promise<PackageResponse> {
  const msgId = uuid.v4()

  const fetchingManifest = waiters['add'](`manifestResponse:${msgId}`) // tslint:disable-line
  const fetchingFiles = waiters['add'](`packageFilesResponse:${msgId}`) // tslint:disable-line
  const response = waiters['add'](`packageResponse:${msgId}`) // tslint:disable-line
    .then((packageResponse: object) => {
      return Object.assign(packageResponse, {
        fetchingFiles,
        fetchingManifest,
        finishing: Promise.all([fetchingManifest, fetchingFiles]).then(() => undefined),
      })
    })

  socket.sendMessage({
    action: 'requestPackage',
    args: [wantedDependency, options],
    msgId,
  }, (err) => err && console.error(err))

  return response
}
