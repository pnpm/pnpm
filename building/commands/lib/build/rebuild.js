import { buildProjects, buildSelectedPkgs, } from '@pnpm/building.after-install';
import { FILTERING, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help';
import { docsUrl, readProjectManifestOnly } from '@pnpm/cli.utils';
import { types as allTypes } from '@pnpm/config.reader';
import { createStoreController, } from '@pnpm/store.connection-manager';
import { pick } from 'ramda';
import { renderHelp } from 'render-help';
import { recursiveRebuild } from './recursive.js';
export function rcOptionsTypes() {
    return {
        ...pick([
            'npm-path',
            'reporter',
            'scripts-prepend-node-path',
            'unsafe-perm',
            'store-dir',
        ], allTypes),
    };
}
export function cliOptionsTypes() {
    return {
        ...rcOptionsTypes(),
        pending: Boolean,
        recursive: Boolean,
    };
}
export const commandNames = ['rebuild', 'rb'];
export function help() {
    return renderHelp({
        aliases: ['rb'],
        description: 'Rebuild a package.',
        descriptionLists: [
            {
                title: 'Options',
                list: [
                    {
                        description: 'Rebuild every package found in subdirectories \
or every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
                        name: '--recursive',
                        shortAlias: '-r',
                    },
                    {
                        description: 'Rebuild packages that were not built during installation. Packages are not built when installing with the --ignore-scripts flag',
                        name: '--pending',
                    },
                    {
                        description: 'The directory in which all the packages are saved on the disk',
                        name: '--store-dir <dir>',
                    },
                    ...UNIVERSAL_OPTIONS,
                ],
            },
            FILTERING,
        ],
        url: docsUrl('rebuild'),
        usages: ['pnpm rebuild [<pkg> ...]'],
    });
}
export async function handler(opts, params) {
    if (opts.recursive && (opts.allProjects != null) && (opts.selectedProjectsGraph != null) && opts.workspaceDir) {
        await recursiveRebuild(opts.allProjects, params, { ...opts, selectedProjectsGraph: opts.selectedProjectsGraph, workspaceDir: opts.workspaceDir });
        return;
    }
    const store = await createStoreController(opts);
    const rebuildOpts = Object.assign(opts, {
        sideEffectsCacheRead: opts.sideEffectsCache ?? opts.sideEffectsCacheReadonly,
        sideEffectsCacheWrite: opts.sideEffectsCache,
        storeController: store.ctrl,
        storeDir: store.dir,
    });
    if (params.length === 0) {
        await buildProjects([
            {
                buildIndex: 0,
                manifest: await readProjectManifestOnly(rebuildOpts.dir, opts),
                rootDir: rebuildOpts.dir,
            },
        ], rebuildOpts);
        return;
    }
    await buildSelectedPkgs([
        {
            buildIndex: 0,
            manifest: await readProjectManifestOnly(rebuildOpts.dir, opts),
            rootDir: rebuildOpts.dir,
        },
    ], params, rebuildOpts);
}
//# sourceMappingURL=rebuild.js.map