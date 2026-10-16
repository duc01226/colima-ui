import { getContext, onDestroy, setContext, untrack } from "svelte";

interface RecoveryScope {
  key: string;
  revision: number | null;
  suspended: boolean;
}
interface DraftOwner {
  scope: () => RecoveryScope | null;
  take: (scope: RecoveryScope, name: string) => unknown;
  register: (scope: RecoveryScope, name: string, read: () => unknown) => () => void;
}
const contextKey = Symbol("recovery-drafts");

/** App-owned memory only. Callers supply identity and neutral field snapshots;
 * resource data, operations and persisted state never belong in this cache. */
export function createRecoveryDrafts(currentScope: () => RecoveryScope | null) {
  const values = new Map<string, unknown>();
  const registrations = new Map<string, { scope: RecoveryScope; read: () => unknown }>();
  let key: string | null = null;
  let closed = false;
  function reconcile() {
    const next = currentScope()?.key ?? null;
    if (key !== next) { values.clear(); key = next; }
  }
  // Snapshot before render: keyed replacements may mount before old teardown,
  // and teardown can expose pre-loss values. Ordinary dialog Close is not loss.
  $effect.pre(() => {
    const current = currentScope();
    reconcile();
    if (closed || !current) return;
    for (const [name, { scope, read }] of registrations) {
      if (current.key !== scope.key || (!current.suspended && current.revision === scope.revision)) continue;
      untrack(() => { values.set(name, $state.snapshot(read())); });
    }
  });
  setContext<DraftOwner>(contextKey, {
    scope: currentScope,
    take(scope, name) {
      reconcile();
      if (closed || key !== scope.key) return undefined;
      const value = values.get(name);
      values.delete(name);
      return value;
    },
    register(scope, name, read) {
      if (closed) return () => {};
      const registration = { scope, read };
      registrations.set(name, registration);
      return () => {
        if (registrations.get(name) === registration) registrations.delete(name);
      };
    },
  });
  onDestroy(() => { closed = true; values.clear(); registrations.clear(); });
  return { reconcile };
}

/** Optional so a standalone form keeps its ordinary local lifetime. Fixed
 * registration names bound the cache to the forms on one active page. */
export function recoveryDraft<T>(name: string, read: () => T, restore: (draft: T) => void) {
  const owner = getContext<DraftOwner | undefined>(contextKey);
  if (!owner) return;
  // The scope is captured at mount, before availability or revision changes.
  // It is supplied through the context to avoid a module-global draft store.
  const scope = owner.scope();
  if (!scope || scope.suspended) return;
  const saved = owner.take(scope, name);
  if (saved !== undefined) restore(saved as T);
  onDestroy(owner.register(scope, name, read));
}

