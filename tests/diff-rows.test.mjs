import { test } from 'node:test';
import assert from 'node:assert/strict';
import { patchRows, splitPatchRows } from '../src/diffRows.ts';

test('split diff aligns replacements and preserves both line numbers', () => {
  const patch = '--- left\n+++ right\n@@ -9,4 +9,5 @@\n 相同\n-旧一\n-旧二\n+新一\n+新二\n+新增\n 末尾\n';
  const rows = splitPatchRows(patch);
  assert.deepEqual(rows.slice(1).map(r => [r.left?.text, r.left?.left, r.right?.text, r.right?.right]), [
    ['相同','9','相同','9'], ['旧一','10','新一','10'], ['旧二','11','新二','11'], [undefined,undefined,'新增','12'], ['末尾','12','末尾','13'],
  ]);
});
test('separate hunks reset line numbers and added or removed files have an empty side', () => {
  const rows = splitPatchRows('--- left\n+++ right\n@@ -0,0 +1,2 @@\n+first\n+\n@@ -20,1 +30,0 @@\n-gone\n');
  assert.equal(rows[1].left, undefined); assert.equal(rows[2].right.text, '');
  assert.equal(rows[4].left.left, '20'); assert.equal(rows[4].right, undefined);
});
test('newline markers and content resembling diff headers remain visible', () => {
  const patch = '--- left\n+++ right\n@@ -1 +1 @@\n--- content\n+++ content\n\\ No newline at end of file\n';
  const rows = patchRows(patch);
  assert.equal(rows[1].text, '-- content'); assert.equal(rows[2].text, '++ content');
  assert.equal(rows[3].text, '此处文件末尾没有换行符');
  assert.ok(splitPatchRows(patch).some(r => r.note?.includes('没有换行符')));
  assert.deepEqual(splitPatchRows(''), []);
});
