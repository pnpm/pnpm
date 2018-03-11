// TODO: move to separate package. It is used in supi/lib/install.ts as well

import logger from '@pnpm/logger'
import pLimit = require('p-limit')
import {StoreController} from 'package-store'
import R = require('ramda')
import postInstall from 'supi/lib/install/postInstall'
import {DepGraphNodesByDepPath} from '.'
import {ENGINE_NAME} from './constants'

export default async (
  depGraph: DepGraphNodesByDepPath,
  opts: {
    childConcurrency: number,
    prefix: string,
    rawNpmConfig: object,
    unsafePerm: boolean,
    userAgent: string,
    sideEffectsCache: boolean,
    sideEffectsCacheReadonly: boolean,
    storeController: StoreController,
  },
) => {
  // postinstall hooks
  const limitChild = pLimit(opts.childConcurrency || 4)
  // TODO: run depencies first then dependents. Use graph-sequencer to sort the graph
  await Promise.all(
    R.keys(depGraph)
      .filter((depPath) => !depGraph[depPath].isBuilt)
      .map((depPath) => limitChild(async () => {
        const depNode = depGraph[depPath]
        try {
          const hasSideEffects = await postInstall(depNode.peripheralLocation, {
            initialWD: opts.prefix,
            pkgId: depPath, // TODO: postInstall should expect depPath, not pkgId
            rawNpmConfig: opts.rawNpmConfig,
            unsafePerm: opts.unsafePerm || false,
            userAgent: opts.userAgent,
          })
          if (hasSideEffects && opts.sideEffectsCache && !opts.sideEffectsCacheReadonly) {
            try {
              await opts.storeController.upload(depNode.peripheralLocation, {
                engine: ENGINE_NAME,
                pkgId: depNode.pkgId,
              })
            } catch (err) {
              if (err && err.statusCode === 403) {
                logger.warn(`The store server disabled upload requests, could not upload ${depNode.pkgId}`)
              } else {
                logger.warn({
                  err,
                  message: `An error occurred while uploading ${depNode.pkgId}`,
                })
              }
            }
          }
        } catch (err) {
          if (depNode.optional) {
            logger.warn({
              err,
              message: `Skipping failed optional dependency ${depNode.pkgId}`,
            })
            return
          }
          throw err
        }
      }),
    ),
  )
}
