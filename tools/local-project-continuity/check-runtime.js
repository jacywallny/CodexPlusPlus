(async () => {
  const candidates = new Set();
  for (const script of document.querySelectorAll('script[src]')) {
    if (!script.src.startsWith('app://-/assets/')) continue;
    const runtime = (await import(script.src)).__webpack_require__;
    for (const cached of Object.values(runtime?.c || {})) {
      for (const value of Object.values(cached.exports || {})) {
        if (value && typeof value === 'object' && Object.hasOwn(value, 'projects')
            && Object.hasOwn(value, 'applicationState') && Object.hasOwn(value, 'threadProjectAssignments')
            && typeof value.projects.getLocalProjects === 'function'
            && typeof value.applicationState.read === 'function') candidates.add(value);
      }
    }
  }
  if (candidates.size !== 1) throw Error(`Expected unique native service, found ${candidates.size}`);
  const services = [...candidates][0];
  const keys = ['electron-account-renderer-state-v1', 'thread-project-assignments', 'project-order',
    'pinned-project-ids', 'sidebar-project-thread-orders', 'electron-persisted-atom-state'];
  const pairs = await Promise.all(keys.map(async key => [key, (await services.applicationState.read(key)).value]));
  const state = Object.fromEntries(pairs);
  const account = state['electron-account-renderer-state-v1'];
  const projects = await services.projects.getLocalProjects();
  const accountAfter = (await services.applicationState.read('electron-account-renderer-state-v1')).value;
  if (JSON.stringify(account?.owner) !== JSON.stringify(accountAfter?.owner)) throw Error('Identity changed during read-only check');
  const stable = value => Array.isArray(value) ? value.map(stable)
    : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(key => [key, stable(value[key])])) : value;
  const hash = async value => Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',
    new TextEncoder().encode(JSON.stringify(stable(value))))), byte => byte.toString(16).padStart(2, '0')).join('');
  const assignments = state['thread-project-assignments'] || {};
  const atoms = state['electron-persisted-atom-state'] || {};
  const continuity = window.__codexLocalProjectContinuity;
  return {capturedAt: new Date().toISOString(), pageBootAt: new Date(performance.timeOrigin).toISOString(),
    ownerFingerprint: account?.owner ?? null, inactiveOwnerCount: Object.keys(account?.inactive || {}).length,
    continuity: continuity ? Object.fromEntries(Object.entries(continuity).filter(([, value]) => typeof value !== 'function')) : null,
    native: {projects: Object.keys(projects).length, assignments: Object.keys(assignments).length,
      projectOrder: (state['project-order'] || []).length, pinnedProjects: (state['pinned-project-ids'] || []).length,
      sidebarOrders: Object.keys(state['sidebar-project-thread-orders'] || {}).length,
      projectIdsHash: await hash(Object.keys(projects).sort()), projectsHash: await hash(projects),
      assignmentsHash: await hash(assignments), projectOrderHash: await hash(state['project-order'] || []),
      sidebarOrdersHash: await hash(state['sidebar-project-thread-orders'] || {})},
    onboarding: {pending: atoms['electron:onboarding-welcome-pending'] ?? null,
      lastCompleted: atoms.last_completed_onboarding ?? null,
      projectlessCompleted: atoms['electron:onboarding-projectless-completed'] ?? null,
      conversationalCompleted: atoms['electron:onboarding-welcome-v2-role-state']?.completedConversationalOnboarding ?? null,
      roles: atoms['electron:onboarding-welcome-v2-role-state']?.roles ?? null,
      workMode: atoms['electron:onboarding-welcome-v2-role-state']?.workMode ?? null}};
})()
