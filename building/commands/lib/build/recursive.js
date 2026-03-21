import assert from 'node:assert';
import util from 'node:util';
import { buildProjects as rebuildAll, buildSelectedPkgs } from '@pnpm/building.after-install';
import { throwOnCommandFail, } from '@pnpm/cli.utils';
import { createProjectConfigRecord, getWorkspaceConcurrency, } from '@pnpm/config.reader';
import { logger } from '@pnpm/logger';
import { createStoreController } from '@pnpm/store.connection-manager';
import { sortProjects } from '@pnpm/workspace.projects-sorter';
import pLimit from 'p-limit';
export async function recursiveRebuild(allProjects, params, opts) {
    if (allProjects.length === 0) {
        // It might make sense to throw an exception in this case
        return;
    }
    const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package);
    if (pkgs.length === 0) {
        return;
    }
    const manifestsByPath = {};
    for (const { rootDir, manifest, writeProjectManifest } of pkgs) {
        manifestsByPath[rootDir] = { manifest, writeProjectManifest };
    }
    const throwOnFail = throwOnCommandFail.bind(null, 'pnpm recursive rebuild');
    const chunks = opts.sort !== false
        ? sortProjects(opts.selectedProjectsGraph)
        : [Object.keys(opts.selectedProjectsGraph).sort()];
    const store = await createStoreController(opts);
    const rebuildOpts = Object.assign(opts, {
        ownLifecycleHooksStdio: 'pipe',
        pruneLockfileImporters: ((opts.ignoredPackages == null) || opts.ignoredPackages.size === 0) &&
            pkgs.length === allProjects.length,
        storeController: store.ctrl,
        storeDir: store.dir,
    });
    const result = {};
    const projectConfigRecord = createProjectConfigRecord(opts) ?? {};
    async function getImporters() {
        const importers = [];
        await Promise.all(chunks.map(async (prefixes, buildIndex) => {
            if (opts.ignoredPackages != null) {
                prefixes = prefixes.filter((prefix) => !opts.ignoredPackages.has(prefix));
            }
            return Promise.all(prefixes.map(async (prefix) => {
                importers.push({
                    buildIndex,
                    manifest: manifestsByPath[prefix].manifest,
                    rootDir: prefix,
                });
            }));
        }));
        return importers;
    }
    const rebuild = (params.length === 0
        ? rebuildAll
        : (importers, opts) => buildSelectedPkgs(importers, params, opts) // eslint-disable-line
    );
    if (opts.lockfileDir) {
        const importers = await getImporters();
        await rebuild(importers, {
            ...rebuildOpts,
            pending: opts.pending === true,
        });
        return;
    }
    const limitRebuild = pLimit(getWorkspaceConcurrency(opts.workspaceConcurrency));
    for (const chunk of chunks) {
        // eslint-disable-next-line no-await-in-loop
        await Promise.all(chunk.map(async (rootDir) => limitRebuild(async () => {
            try {
                if (opts.ignoredPackages?.has(rootDir)) {
                    return;
                }
                result[rootDir] = { status: 'running' };
                const { manifest } = opts.selectedProjectsGraph[rootDir].package;
                const localConfig = manifest.name ? projectConfigRecord[manifest.name] : undefined;
                await rebuild([
                    {
                        buildIndex: 0,
                        manifest: manifestsByPath[rootDir].manifest,
                        rootDir,
                    },
                ], {
                    ...rebuildOpts,
                    ...localConfig,
                    dir: rootDir,
                    pending: opts.pending === true,
                    rawConfig: {
                        ...rebuildOpts.rawConfig,
                        ...localConfig,
                    },
                });
                result[rootDir].status = 'passed';
            }
            catch (err) {
                assert(util.types.isNativeError(err));
                const errWithPrefix = Object.assign(err, {
                    prefix: rootDir,
                });
                logger.info(errWithPrefix);
                if (!opts.bail) {
                    result[rootDir] = {
                        status: 'failure',
                        error: errWithPrefix,
                        message: err.message,
                        prefix: rootDir,
                    };
                    return;
                }
                throw err;
            }
        })));
    }
    throwOnFail(result);
}
//# sourceMappingURL=recursive.js.map