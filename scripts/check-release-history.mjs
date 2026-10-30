import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export function checkReleaseHistory(root = process.cwd(), version = JSON.parse(readFileSync(resolve(root, 'frontend/package.json'), 'utf8')).version, target = 'HEAD') {
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
  assert.equal(git('rev-parse', '--is-shallow-repository'), 'false', 'Fetch full history and tags before checking release ancestry');
  assert.match(version, /^\d+\.\d+\.\d+$/);
  const current = version.split('.').map(Number);
  const older = git('tag', '--list', 'v*').split('\n').filter(tag => {
    if (!/^v\d+\.\d+\.\d+$/.test(tag)) return false;
    const parts = tag.slice(1).split('.').map(Number);
    for (let i = 0; i < 3; i++) {
      if (parts[i] !== current[i]) return parts[i] < current[i];
    }
    return false;
  });
  assert.ok(older.length, 'Expected previous release tags; fetch tags before releasing');
  const missing = older.filter(tag => spawnSync('git', ['merge-base', '--is-ancestor', tag, target], { cwd: root }).status !== 0);
  assert.deepEqual(missing, [], `Release ${version} omits previous release history: ${missing.join(', ')}`);
  return older;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const included = checkReleaseHistory();
  console.log(`Release ancestry includes ${included.join(', ')}`);
}
