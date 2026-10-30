import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { checkReleaseHistory } from '../check-release-history.mjs';

test('release must include every older release, including a diverged feature line', () => {
  const root = mkdtempSync(join(tmpdir(), 'mpc-release-history-'));
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  try {
    git('init', '-b', 'main');
    git('config', 'user.name', 'Release test');
    git('config', 'user.email', 'release@example.invalid');
    git('commit', '--allow-empty', '-m', 'base');
    git('tag', 'v0.1.0');
    git('switch', '-c', 'features');
    git('commit', '--allow-empty', '-m', 'themes and protections');
    git('tag', 'v0.1.2');
    git('switch', 'main');
    git('commit', '--allow-empty', '-m', 'drafts');
    git('tag', 'v0.1.5');
    assert.throws(() => checkReleaseHistory(root, '0.1.6'), /v0.1.2/);
    git('merge', '--no-ff', 'features', '-m', 'reconcile');
    assert.deepEqual(checkReleaseHistory(root, '0.1.6'), ['v0.1.0', 'v0.1.2', 'v0.1.5']);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
