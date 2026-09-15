import test from 'node:test';
import assert from 'node:assert/strict';
import { unified } from 'unified';
import remarkParse from 'remark-parse';
import remarkGfm from 'remark-gfm';
import { sentences, combineTranslation, bilingualPlugin, documentParts } from '../src/translation.ts';

test('sentence boundaries preserve Markdown, metadata, links and code blocks', () => {
  const source = '\uFEFF---\r\nname: demo\r\n---\r\n# Usage\n\nRead `foo.bar()` first. Then open [the guide](https://example.com/a.b).\n\n- Keep this rule. Follow it.\n\n```js\nconsole.log("Not translated.");\n```\n';
  const parts = sentences(source);
  assert.deepEqual(parts.map(p => p.original), ['Usage', 'Read `foo.bar()` first.', 'Then open [the guide](https://example.com/a.b).', 'Keep this rule.', 'Follow it.']);
  const pairs = parts.map((p, i) => ({...p, translated: ['用法', '先读 `foo.bar()`。', '然后打开 [指南](https://example.com/a.b)。', '遵守此规则。', '执行它。'][i]}));
  const output = combineTranslation(source, pairs);
  assert.ok(output.startsWith('\uFEFF---\r\nname: demo\r\n---\r\n# 用法'));
  assert.ok(output.includes('- 遵守此规则。 执行它。'));
  assert.ok(output.includes('```js\nconsole.log("Not translated.");\n```'));
  assert.equal(source.includes('用法'), false);
});

test('missing or reordered translations and modified code cannot produce a replacement', () => {
  const source = 'Keep `name`. Read [guide](https://example.com).';
  const pairs = sentences(source).map(p => ({...p, translated:p.original}));
  assert.throws(() => combineTranslation(source, pairs.slice(1)), /句数/);
  assert.throws(() => combineTranslation(source, [...pairs].reverse()), /编号/);
  assert.throws(() => combineTranslation(source, [{...pairs[0], translated:'保留 `other`。'}, pairs[1]]), /代码或链接/);
  assert.throws(() => combineTranslation(source, [pairs[0], {...pairs[1], translated:'[指南](https://other.com)'}]), /代码或链接/);
});

test('bilingual rendering retains lists, tables and code with each translation under its original', async () => {
  const source = '# Hello\n\n- First sentence. Second sentence!\n\n| Name | Purpose |\n| --- | --- |\n| Tool | Translate words. |\n\n```sh\necho hello\n```\n';
  const pairs = sentences(source).map(p => ({...p, translated:`译文 ${p.id}`}));
  const processor = unified().use(remarkParse).use(remarkGfm).use(bilingualPlugin(source, pairs));
  const tree = await processor.run(processor.parse(documentParts(source).body));
  assert.deepEqual(tree.children.map(n => n.type), ['heading','list','table','code']);
  const renderedPairs = [];
  function visit(node) {
    if (node.data?.hProperties?.className?.includes('sentence-pair')) renderedPairs.push(node);
    for (const child of node.children ?? []) visit(child);
  }
  visit(tree);
  assert.equal(renderedPairs.length, pairs.length);
  for (let i = 0; i < pairs.length; i++) {
    assert.deepEqual(renderedPairs[i].children.map(n => n.data.hProperties.className[0]), ['sentence-original', 'sentence-translated']);
    assert.equal(renderedPairs[i].children[1].children[0].value, pairs[i].translated);
  }
  assert.equal(tree.children.at(-1).value, 'echo hello');
});

test('code-only document is unchanged and has no translation requests', () => {
  const source = '---\nname: demo\n---\n```js\nconst x = 1;\n```\n\n`identifier`\n';
  assert.deepEqual(sentences(source), []);
  assert.equal(combineTranslation(source, []), source);
});
