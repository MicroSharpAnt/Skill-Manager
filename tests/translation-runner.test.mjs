import test from 'node:test';
import assert from 'node:assert/strict';
import {runTranslation, partialTranslation} from '../src/translationRunner.ts';
const source = Array.from({length:24}, (_,i)=>`Sentence number ${i+1}.`).join('\n\n');
const tick = () => new Promise(resolve => setImmediate(resolve));
const translated = batch => batch.map(s=>({id:s.id,text:`译文 ${s.id}。`}));

test('two batches run concurrently and partial output appears before remaining requests finish', async () => {
  const pending = [], updates = [];
  const done = runTranslation(source, batch => new Promise(resolve=>pending.push({batch,resolve})), pairs=>updates.push(pairs), ()=>false);
  assert.equal(pending.length,2);
  pending[1].resolve(translated(pending[1].batch));
  await tick();
  assert.equal(updates.at(-1).length,8);
  assert.match(partialTranslation(source,updates.at(-1)), /Sentence number 1\./);
  assert.match(partialTranslation(source,updates.at(-1)), /译文/);
  assert.equal(pending.length,3);
  pending[2].resolve(translated(pending[2].batch));
  pending[0].resolve(translated(pending[0].batch));
  const result = await done;
  assert.equal(result.pairs.length,24);
  assert.deepEqual(result.pairs.map(p=>p.id), [...result.pairs.map(p=>p.id)].sort((a,b)=>a-b));
  assert.ok(!result.text.includes('Sentence number'));
});

test('stopping waiting schedules no more batches and publishes no stale result', async () => {
  const pending = [], updates = []; let cancelled = false;
  const done = runTranslation(source, batch => new Promise(resolve=>pending.push({batch,resolve})), p=>updates.push(p), ()=>cancelled);
  const rejected = assert.rejects(done,/已停止等待/);
  cancelled = true;
  for (const p of pending) p.resolve(translated(p.batch));
  await rejected;
  assert.equal(pending.length,2);
  assert.equal(updates.length,1);
});

test('invalid translation fails without starting further batches', async () => {
  let calls = 0;
  await assert.rejects(runTranslation(source, async ()=>{ calls++; return []; }, ()=>{}, ()=>false), /编号不完整/);
  assert.equal(calls,2);
});
