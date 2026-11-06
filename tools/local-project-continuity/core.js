(function (root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.__CodexLocalProjectContinuityCore = api;
})(globalThis, function () {
  'use strict';
  const object = value => value && typeof value === 'object' && !Array.isArray(value) ? value : {};
  const own = (value, key) => Object.hasOwn(object(value), key);
  const clone = value => JSON.parse(JSON.stringify(value));
  const safeId = id => typeof id === 'string' && id.length > 0 && !['__proto__', 'constructor', 'prototype'].includes(id);
  const PROJECTS = 'local-projects';
  const ASSIGNMENTS = 'thread-project-assignments';
  const GLOBAL_KEYS = [PROJECTS, 'project-order', 'pinned-project-ids', 'project-appearances',
    'electron-saved-workspace-roots', 'electron-workspace-root-labels',
    'app-server-project-id-by-legacy-project-id-by-host', 'app-server-projects-migration-by-host',
    'sidebar-project-thread-orders', ASSIGNMENTS, 'thread-project-membership-host-ids', 'projectless-thread-ids'];
  const ATOM_KEYS = ['last_completed_onboarding', 'electron:onboarding-projectless-completed',
    'electron:onboarding-welcome-v2-role-state', 'electron:onboarding-welcome-pending'];
  const plainProject = (id, value) => safeId(id) && value && value.id === id
    && typeof value.name === 'string' && Array.isArray(value.rootPaths) && value.rootPaths.length > 0
    && value.rootPaths.every(path => typeof path === 'string' && path.length > 0)
    && !value.chatGptBacking && !value.chatGptBackingRecoveryPending;
  const unique = values => [...new Set(values.filter(value => typeof value === 'string'))];
  function mergeHostMaps(older, newer) {
    const result={...object(older)};
    for(const [host,mapping] of Object.entries(object(newer))) result[host]={...object(result[host]),...object(mapping)};
    return result;
  }

  function onboarding(atoms) {
    atoms = object(atoms);
    const role = object(atoms['electron:onboarding-welcome-v2-role-state']);
    const completed = Number(atoms.last_completed_onboarding) > 0
      || atoms['electron:onboarding-projectless-completed'] === true
      || role.completedConversationalOnboarding === true;
    if (!completed) return {};
    const result = { 'electron:onboarding-welcome-pending': false };
    for (const key of ['last_completed_onboarding', 'electron:onboarding-projectless-completed']) {
      if (own(atoms, key)) result[key] = clone(atoms[key]);
    }
    if (Object.keys(role).length) {
      result['electron:onboarding-welcome-v2-role-state'] = Object.fromEntries(
        ['roles', 'personalizedSuggestionsEnabled', 'workMode', 'completedConversationalOnboarding', 'conversationalOnboardingSkipped']
          .filter(key => own(role, key)).map(key => [key, clone(role[key])]),
      );
    }
    return result;
  }

  function sanitize(globals, atoms, validThreadIds) {
    globals = object(globals);
    const valid = new Set(validThreadIds);
    const projects = Object.fromEntries(Object.entries(object(globals[PROJECTS])).filter(([id, project]) => plainProject(id, project)));
    const ids = new Set(Object.keys(projects));
    const presentationIds=new Set(ids);
    for(const mapping of Object.values(object(globals['app-server-project-id-by-legacy-project-id-by-host']))) {
      for(const [legacyId,serverId] of Object.entries(object(mapping))) if(ids.has(legacyId) && typeof serverId==='string') presentationIds.add(serverId);
    }
    const assignments = Object.fromEntries(Object.entries(object(globals[ASSIGNMENTS])).filter(([id, assignment]) =>
      valid.has(id) && assignment?.projectKind === 'local' && ids.has(assignment.projectId)));
    const result = { [PROJECTS]: clone(projects), [ASSIGNMENTS]: clone(assignments) };
    for (const key of ['project-order', 'pinned-project-ids']) {
      result[key] = unique(Array.isArray(globals[key]) ? globals[key] : []).filter(id => ids.has(id));
    }
    result['project-order'] = unique([...result['project-order'], ...ids]);
    for (const key of ['project-appearances', 'sidebar-project-thread-orders']) {
      result[key] = Object.fromEntries(Object.entries(object(globals[key])).filter(([id]) => presentationIds.has(id)));
    }
    result['thread-project-membership-host-ids'] = Object.fromEntries(
      Object.entries(object(globals['thread-project-membership-host-ids'])).filter(([id, host]) => valid.has(id) && typeof host === 'string'),
    );
    result['projectless-thread-ids'] = unique(Array.isArray(globals['projectless-thread-ids']) ? globals['projectless-thread-ids'] : []).filter(id => valid.has(id));
    const roots = unique(Object.values(projects).flatMap(project => project.rootPaths));
    result['electron-saved-workspace-roots'] = unique([...(Array.isArray(globals['electron-saved-workspace-roots']) ? globals['electron-saved-workspace-roots'] : []), ...roots]);
    result['electron-workspace-root-labels'] = Object.fromEntries(Object.entries(object(globals['electron-workspace-root-labels'])).filter(([path]) => roots.includes(path)));
    for (const key of ['app-server-project-id-by-legacy-project-id-by-host', 'app-server-projects-migration-by-host']) {
      result[key] = Object.fromEntries(Object.entries(object(globals[key])).filter(([host]) => host.startsWith('local:')));
    }
    return { globals: result, atoms: onboarding(atoms) };
  }

  function mergeSnapshots(older, newer) {
    const result = {globals: {}, atoms: {...object(older?.atoms), ...object(newer?.atoms)}};
    for (const key of GLOBAL_KEYS) {
      const old = older?.globals?.[key], next = newer?.globals?.[key];
      if (Array.isArray(old) || Array.isArray(next)) result.globals[key] = unique([...(next || []), ...(old || [])]);
      else if(key==='app-server-project-id-by-legacy-project-id-by-host') result.globals[key]=mergeHostMaps(old,next);
      else result.globals[key] = {...object(old), ...object(next)};
    }
    return result;
  }

  function seed(state, validThreadIds) {
    let result = {globals:{},atoms:{}};
    for (const scope of Object.values(object(state['electron-account-renderer-state-v1']?.inactive))) {
      result = mergeSnapshots(result, sanitize(scope.globals, scope.atoms, validThreadIds));
    }
    result = mergeSnapshots(result, sanitize(state, state['electron-persisted-atom-state'], validThreadIds));
    return sanitize(result.globals, result.atoms, validThreadIds);
  }

  function plan(snapshot, current) {
    const updates = {};
    const currentProjects = object(current.globals?.[PROJECTS]);
    const projects = {...object(snapshot.globals?.[PROJECTS]), ...currentProjects};
    for (const key of GLOBAL_KEYS) {
      if ([ASSIGNMENTS, 'projectless-thread-ids', 'project-order'].includes(key)) continue;
      const old = snapshot.globals?.[key], live = current.globals?.[key];
      let value;
      if (key === PROJECTS) value = projects;
      else if (Array.isArray(old) || Array.isArray(live)) value = unique([...(live || []), ...(old || [])]);
      else if(key==='app-server-project-id-by-legacy-project-id-by-host') value=mergeHostMaps(old,live);
      else value = {...object(old), ...object(live)};
      if (JSON.stringify(value) !== JSON.stringify(live ?? (Array.isArray(value) ? [] : {}))) updates[key] = value;
    }
    const projectless = new Set(current.globals?.['projectless-thread-ids'] || []);
    const assignments = Object.entries(object(snapshot.globals?.[ASSIGNMENTS])).filter(([id, assignment]) =>
      !own(current.globals?.[ASSIGNMENTS], id) && !projectless.has(id) && own(projects, assignment.projectId))
      .map(([threadId, assignment]) => ({threadId, hostId: snapshot.globals?.['thread-project-membership-host-ids']?.[threadId] || 'local', projectless:false, assignment}));
    const order = unique([...(current.globals?.['project-order'] || []), ...(snapshot.globals?.['project-order'] || []), ...Object.keys(projects)]);
    const atoms = {...object(current.atoms), ...onboarding(snapshot.atoms)};
    if (Object.keys(onboarding(snapshot.atoms)).length) {
      const liveRole = object(current.atoms?.['electron:onboarding-welcome-v2-role-state']);
      if (Object.keys(liveRole).length || own(atoms,'electron:onboarding-welcome-v2-role-state')) {
        const savedRole=object(atoms['electron:onboarding-welcome-v2-role-state']);
        const role={...savedRole,...liveRole};
        if(!Array.isArray(liveRole.roles) || !liveRole.roles.length) {if(savedRole.roles) role.roles=clone(savedRole.roles);}
        if(liveRole.workMode==null && savedRole.workMode!=null) role.workMode=savedRole.workMode;
        if(savedRole.completedConversationalOnboarding===true) role.completedConversationalOnboarding=true;
        atoms['electron:onboarding-welcome-v2-role-state']=role;
      }
      if(Number(current.atoms?.last_completed_onboarding)>Number(atoms.last_completed_onboarding || 0)) atoms.last_completed_onboarding=current.atoms.last_completed_onboarding;
      atoms['electron:onboarding-welcome-pending'] = false;
    }
    return {updates, assignments, order, atoms, atomsChanged: JSON.stringify(atoms) !== JSON.stringify(current.atoms || {}),
      projectCount:Object.keys(projects).length};
  }

  return {GLOBAL_KEYS, ATOM_KEYS, seed, sanitize, plan, mergeSnapshots, plainProject};
});
