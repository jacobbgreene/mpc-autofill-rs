import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import test from 'node:test';
import { collectSource } from '../collect-source.mjs';

const tar = process.platform === 'win32'
  ? join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe')
  : 'tar';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'mpc-source-test-'));
  t.after(() => {
    assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep));
    rmSync(root, { recursive: true, force: true });
  });
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  mkdirSync(join(root, 'frontend'));
  writeFileSync(join(root, '.gitignore'), '/dist\n');
  writeFileSync(join(root, 'frontend/package.json'), JSON.stringify({ version: '1.2.3' }));
  writeFileSync(join(root, 'LICENSE'), 'Fixture license text\n');
  writeFileSync(join(root, 'NOTICE.md'), 'Fixture upstream credit\n');
  writeFileSync(join(root, 'README.md'), 'Fixture build instructions\n');
  writeFileSync(join(root, 'Cargo.lock'), '# Fixture lockfile\n');
  git('init');
  git('config', 'core.autocrlf', 'false');
  git('add', '.');
  git('-c', 'user.name=Source Test', '-c', 'user.email=source-test@example.invalid', 'commit', '-m', 'Fixture');
  return { root, git };
}

test('source bundle matches its commit and excludes untracked local files', t => {
  const { root, git } = fixture(t);
  writeFileSync(join(root, 'untracked.txt'), 'Local scratch data');
  const { archive, commit, directory } = collectSource(root);
  assert.equal(commit, git('rev-parse', 'HEAD').trim());
  const metadata = JSON.parse(readFileSync(join(directory, 'SOURCE.json'), 'utf8'));
  assert.equal(metadata.commit, commit);
  assert.equal(metadata.version, '1.2.3');
  assert.equal(metadata.archive, archive);
  const archivePath = join(directory, archive);
  const entries = execFileSync(tar, ['-tzf', archivePath], { encoding: 'utf8' });
  assert.ok(entries.includes('MPC-Autofill_1.2.3/Cargo.lock'));
  assert.ok(entries.includes('MPC-Autofill_1.2.3/README.md'));
  assert.ok(!entries.includes('untracked.txt'));
  assert.ok(!entries.includes('/.git/'));
  for (const file of ['LICENSE', 'NOTICE.md', 'frontend/package.json']) {
    const content = execFileSync(tar, ['-xOzf', archivePath, `MPC-Autofill_1.2.3/${file}`], { encoding: 'utf8' });
    assert.equal(content, git('show', `HEAD:${file}`));
    if (file !== 'frontend/package.json') {
      assert.equal(readFileSync(join(directory, file), 'utf8'), content);
    }
  }
});

test('source packaging rejects unstaged and staged changes to tracked source', t => {
  const { root, git } = fixture(t);
  writeFileSync(join(root, 'README.md'), 'Uncommitted build instructions\n');
  assert.throws(() => collectSource(root), /Commit tracked changes/);
  git('add', 'README.md');
  assert.throws(() => collectSource(root), /Commit tracked changes/);
});
