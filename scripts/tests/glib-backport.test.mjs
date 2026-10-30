import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('../..', import.meta.url)));

test('backport gate rejects reverted fixes, unrelated source changes, and registry fallback', () => {
  const temporaryRoot = resolve(tmpdir());
  const fixture = mkdtempSync(join(temporaryRoot, 'mpc-backport-proof-'));
  try {
    mkdirSync(join(fixture, 'scripts'));
    cpSync(join(root, 'vendor'), join(fixture, 'vendor'), { recursive: true });
    for (const file of ['Cargo.toml', 'Cargo.lock', 'scripts/check-glib-backport.mjs']) {
      cpSync(join(root, file), join(fixture, file));
    }
    const check = () => spawnSync(process.execPath, [join(fixture, 'scripts/check-glib-backport.mjs')], { encoding: 'utf8' });
    assert.equal(check().status, 0);
    const iterator = join(fixture, 'vendor/glib/src/variant_iter.rs');
    const patched = readFileSync(iterator);
    writeFileSync(iterator, patched.toString('utf8').replace('                &mut p,', '                &p,'));
    assert.notEqual(check().status, 0, 'Reverting the actual fix must block the advisory exception');
    writeFileSync(iterator, patched);

    const otherFile = join(fixture, 'vendor/glib/src/lib.rs');
    const original = readFileSync(otherFile);
    writeFileSync(otherFile, Buffer.concat([original, Buffer.from('\n// unexpected change\n')]));
    assert.notEqual(check().status, 0, 'Unreviewed changes elsewhere in the vendored crate must fail');
    writeFileSync(otherFile, original);

    const lockPath = join(fixture, 'Cargo.lock');
    const lock = readFileSync(lockPath, 'utf8');
    writeFileSync(lockPath, lock.replace(/name = "glib"\r?\nversion = "0\.18\.5"/,
      'name = "glib"\nversion = "0.18.5"\nsource = "registry+https://github.com/rust-lang/crates.io-index"'));
    assert.notEqual(check().status, 0, 'Accidentally restoring vulnerable registry GLib must fail');
  } finally {
    assert.equal(dirname(resolve(fixture)), temporaryRoot);
    assert.ok(basename(fixture).startsWith('mpc-backport-proof-'));
    rmSync(fixture, { recursive: true, force: true });
  }
});
