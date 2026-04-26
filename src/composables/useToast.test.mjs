import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  clearToasts,
  dismissToast,
  pushToast,
  useToast,
} from './useToast.ts';

function resetQueue() {
  clearToasts();
}

test('pushToast returns an id and adds the toast to the queue', () => {
  resetQueue();
  const id = pushToast('hello', 'info', { ttlMs: 0 });
  assert.equal(typeof id, 'string');
  assert.ok(id.length > 0, 'id should not be empty');

  const { toasts } = useToast();
  assert.equal(toasts.value.length, 1);
  assert.equal(toasts.value[0].id, id);
  assert.equal(toasts.value[0].message, 'hello');
  assert.equal(toasts.value[0].tone, 'info');
});

test('pushToast auto-dismisses after the ttl elapses', async () => {
  resetQueue();
  const id = pushToast('flash', 'success', { ttlMs: 25 });
  const { toasts } = useToast();
  assert.equal(toasts.value.length, 1);
  assert.equal(toasts.value[0].id, id);

  await new Promise((resolve) => setTimeout(resolve, 60));
  assert.equal(toasts.value.length, 0, 'toast should auto-dismiss after ttl');
});

test('dismissToast removes the matching id and leaves siblings alone', () => {
  resetQueue();
  const first = pushToast('first', 'info', { ttlMs: 0 });
  const second = pushToast('second', 'info', { ttlMs: 0 });
  const third = pushToast('third', 'info', { ttlMs: 0 });

  dismissToast(second);

  const { toasts } = useToast();
  assert.equal(toasts.value.length, 2);
  const remaining = toasts.value.map((toast) => toast.id);
  assert.deepEqual(remaining, [first, third]);
});

test('clearToasts empties the queue', () => {
  resetQueue();
  pushToast('a', 'info', { ttlMs: 0 });
  pushToast('b', 'warning', { ttlMs: 0 });
  pushToast('c', 'error', { ttlMs: 0 });

  const { toasts } = useToast();
  assert.equal(toasts.value.length, 3);

  clearToasts();
  assert.equal(toasts.value.length, 0);
});

test('pushToast with ttlMs=0 does not auto-dismiss', async () => {
  resetQueue();
  pushToast('persistent', 'warning', { ttlMs: 0 });

  await new Promise((resolve) => setTimeout(resolve, 50));

  const { toasts } = useToast();
  assert.equal(toasts.value.length, 1, 'ttl=0 should keep the toast on screen');
  assert.equal(toasts.value[0].message, 'persistent');
});

test('pushToast preserves the optional action payload', () => {
  resetQueue();
  let clicked = 0;
  pushToast('with-action', 'info', {
    ttlMs: 0,
    action: { label: 'Retry', onClick: () => (clicked += 1) },
  });

  const { toasts } = useToast();
  assert.equal(toasts.value.length, 1);
  const action = toasts.value[0].action;
  assert.ok(action, 'action should be attached');
  assert.equal(action.label, 'Retry');
  action.onClick();
  assert.equal(clicked, 1);
});
