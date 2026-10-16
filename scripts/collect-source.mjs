import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export function collectSource(root = process.cwd()) {
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' });
  // An archive of HEAD must not be presented as the source of a locally modified build.
  assert.equal(git('diff', '--name-only', 'HEAD', '--').trim(), '',
    'Commit tracked changes before packaging release source');
  const commit = git('rev-parse', 'HEAD').trim();
  const { version } = JSON.parse(git('show', 'HEAD:frontend/package.json'));
  assert.match(version, /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/);
  const license = git('show', 'HEAD:LICENSE');
  const notices = git('show', 'HEAD:NOTICE.md');
  const directory = join(root, 'dist');
  const archive = `MPC-Autofill_${version}_source.tar.gz`;
  mkdirSync(directory, { recursive: true });
  git('archive', '--format=tar.gz', `--prefix=MPC-Autofill_${version}/`,
    `--output=${join(directory, archive)}`, commit);
  writeFileSync(join(directory, 'LICENSE'), license);
  writeFileSync(join(directory, 'NOTICE.md'), notices);
  writeFileSync(join(directory, 'SOURCE.json'), JSON.stringify({
    repository: 'https://github.com/jacobbgreene/mpc-autofill-rs',
    version, commit, archive,
    contents: 'Exact tracked repository source, including lockfiles and build scripts. See README.md for build instructions.',
  }, null, 2) + '\n');
  return { archive, commit, directory };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { archive, commit } = collectSource();
  console.log(`Source: ${archive} (${commit})`);
}
