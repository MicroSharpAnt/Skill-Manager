import { test } from 'node:test';
import assert from 'node:assert/strict';
import { groupState, clientGroupState } from '../src/groupState.ts';

test('group toggle distinguishes empty, disabled, enabled and mixed groups', () => {
  assert.equal(groupState([]), 'off');
  assert.equal(groupState([false, false]), 'off');
  assert.equal(groupState([true, true]), 'on');
  assert.equal(groupState([false, true]), 'mixed');
});
test('client groups never count sources or conflicting projections as switchable', () => {
  const skills = ['source', 'conflict', 'modified', 'off', 'link', 'copy'].map((status, i) => ({ id: String(i), clients: { Codex: status } }));
  const state = clientGroupState(skills, 'Codex');
  assert.deepEqual(state.eligible.map(s => s.id), ['3', '4', '5']);
  assert.equal(state.excluded, 3);
  assert.equal(groupState(state.values), 'mixed');
  assert.equal(groupState(clientGroupState(skills, 'Claude').values), 'off');
});
