import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, lstatSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const directory = join(root, 'vendor', 'glib');
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const manifestBytes = readFileSync(join(directory, 'UPSTREAM-INTEGRITY.json'));
assert.equal(digest(manifestBytes), '8a50cb502c6f1ff92f688dcd6190ceef1f1e575e3cb6fbae2cd35192681d5697',
  'Vendored GLib provenance must be explicitly re-reviewed when it changes');
const manifest = JSON.parse(manifestBytes);
assert.equal(manifest.version, '0.18.5');
assert.equal(manifest.upstream_fix, 'b5a4071e439bef2b5eea76c3aa25e5ae84839e34');

for (const [name, originalHash] of Object.entries(manifest.original_files)) {
  const path = resolve(directory, name);
  assert.ok(path.startsWith(directory + '/') || path.startsWith(directory + '\\'), 'Unsafe vendor manifest path');
  assert.ok(lstatSync(path).isFile(), `${name} must be a regular file`);
  const expected = name === manifest.patched_file ? manifest.patched_sha256 : originalHash;
  assert.equal(digest(readFileSync(path)), expected, `Unexpected change in vendored GLib: ${name}`);
}
const extras = new Set(['PATCH.md', 'UPSTREAM-INTEGRITY.json', '.gitattributes']);
function inspect(path) {
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const child = join(path, entry.name);
    assert.ok(!entry.isSymbolicLink(), 'Vendored GLib must not contain symlinks');
    if (entry.isDirectory()) inspect(child);
    else {
      const name = relative(directory, child).split('\\').join('/');
      assert.ok(Object.hasOwn(manifest.original_files, name) || extras.has(name), `Unexpected vendor file: ${name}`);
    }
  }
}
inspect(directory);
assert.equal(readFileSync(join(directory, '.gitattributes'), 'utf8'), '* -text\n');
const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8');
const patches = cargo.split('[patch.crates-io]')[1]?.split(/\r?\n\[/)[0];
assert.match(patches ?? '', /^glib\s*=\s*\{\s*path\s*=\s*"vendor\/glib"\s*\}/m,
  'The workspace must resolve GLib to the verified backport');
const lock = readFileSync(join(root, 'Cargo.lock'), 'utf8');
const glibPackages = lock.split('[[package]]').filter((block) => /^name = "glib"$/m.test(block));
assert.equal(glibPackages.length, 1, 'Every resolved GLib must be the verified backport');
assert.match(glibPackages[0], /^version = "0\.18\.5"$/m);
assert.doesNotMatch(glibPackages[0], /^source\s*=/m, 'GLib must resolve as the verified local path package');
console.log('Verified GLib 0.18.5 with the exact upstream RUSTSEC-2024-0429 backport.');
