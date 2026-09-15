import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolvePreset } from '../src/presets.ts';
const groups=[{id:'a',name:'A',members:['a1','a2']},{id:'b',name:'B',members:['b1']},{id:'c',name:'C',members:['c1']}];
const resources=['a1','a2','b1','c1','loose'].map(id=>({id,name:id,states:{project:true,Codex:true,Claude:true}}));
const preset=(groups={},overrides={},defaultState='keep')=>({id:'one',name:'方案',groups,resources:overrides,defaultState,clients:[]});
test('switching schemes combines groups and ungrouped resource rules',()=>{
  const first=resolvePreset(preset({a:false,b:false},{loose:false}),groups,resources,['project']);
  assert.deepEqual(first.errors,[]); assert.deepEqual(first.changes.map(c=>c.id),['a1','a2','b1','loose']);
  const next=resources.map(r=>({...r,states:{project:!first.changes.some(c=>c.id===r.id)}}));
  const second=resolvePreset(preset({a:true,b:true,c:false}),groups,next,['project']);
  assert.deepEqual(second.changes.map(c=>[c.id,c.enable]),[['a1',true],['a2',true],['b1',true],['c1',false]]);
});
test('individual overrides win and group membership is resolved dynamically',()=>{
  const p=preset({a:false},{a2:true});
  assert.deepEqual(resolvePreset(p,groups,resources,['project']).changes.map(c=>c.id),['a1']);
  const moved=groups.map(g=>g.id==='a'?{...g,members:[...g.members,'loose']}:g);
  assert.deepEqual(resolvePreset(p,moved,resources,['project']).changes.map(c=>c.id),['a1','loose']);
});
test('overlapping groups require an explicit override for conflicting targets',()=>{
  const overlapping=[...groups,{id:'other',name:'Other',members:['a1']}];
  assert.match(resolvePreset(preset({a:false,other:true}),overlapping,resources,['project']).errors[0],/冲突/);
  assert.deepEqual(resolvePreset(preset({a:false,other:true},{a1:true}),overlapping,resources,['project']).errors,[]);
});
test('missing group, individual resource, and stale group member block application',()=>{
  assert.equal(resolvePreset(preset({missing:false},{gone:false}),groups,resources,['project']).errors.length,2);
  assert.match(resolvePreset(preset({a:false}),[{id:'a',name:'A',members:['gone']}],resources,['project']).errors[0],/已不存在/);
});
test('default state and targets apply independently of client and search display',()=>{
  const result=resolvePreset(preset({}, {loose:true},'off'),groups,resources,['Codex']);
  assert.equal(result.changes.length,4);assert.ok(result.changes.every(c=>c.client==='Codex'&&!c.enable));
  assert.equal(resolvePreset(preset(),groups,resources,['project']).changes.length,0);
});
test('unmodifiable client source blocks a configured target and empty targets are invalid',()=>{
  assert.match(resolvePreset(preset({a:false}),groups,[{id:'a1',name:'alpha',states:{Codex:null}},{id:'a2',name:'beta',states:{Codex:true}}],['Codex']).errors[0],/异常状态/);
  assert.match(resolvePreset(preset(),groups,resources,[]).errors[0],/客户端/);
});
test('source may remain enabled through an individual override but cannot be disabled',()=>{
  const rs=[{id:'a1',name:'source',states:{Codex:true},lockedClients:['Codex']},{id:'a2',name:'beta',states:{Codex:true}}];
  assert.match(resolvePreset(preset({a:false}),groups,rs,['Codex']).errors[0],/不能关闭/);
  const result=resolvePreset(preset({a:false},{a1:true}),groups,rs,['Codex']);
  assert.equal(result.errors.length,0);assert.deepEqual(result.changes.map(c=>c.id),['a2']);
});
