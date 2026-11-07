(async () => {
  const candidates = new Set();
  for (const script of document.querySelectorAll('script[src]')) {
    if (!script.src.startsWith('app://-/assets/')) continue;
    const runtime = (await import(script.src)).__webpack_require__;
    for (const cached of Object.values(runtime?.c || {})) {
      for (const value of Object.values(cached.exports || {})) {
        if (value && typeof value === 'object' && Object.hasOwn(value, 'projects')
            && Object.hasOwn(value, 'applicationState') && Object.hasOwn(value, 'threadProjectAssignments')
            && typeof value.projects.getLocalProjects === 'function') candidates.add(value);
      }
    }
  }
  if (candidates.size !== 1) throw Error(`Expected unique native service, found ${candidates.size}`);
  const services = [...candidates][0];
  const core = window.__CodexLocalProjectContinuityCore;
  if (!core) throw Error('Continuity extension is unavailable');
  const read = async key => (await services.applicationState.read(key)).value;
  const account = await read('electron-account-renderer-state-v1');
  const globals = Object.fromEntries(await Promise.all(core.GLOBAL_KEYS.map(async key => [key, await read(key)])));
  const projects = await services.projects.getLocalProjects();
  if (JSON.stringify(Object.keys(projects).sort()) !== JSON.stringify(Object.keys(globals['local-projects'] || {}).sort())) {
    throw Error('Native project list changed during capture');
  }
  const atoms = await read('electron-persisted-atom-state');
  const previousProjects = Object.assign({}, ...Object.values(account?.inactive || {}).map(scope => scope.globals?.['local-projects'] || {}));
  const after = await read('electron-account-renderer-state-v1');
  if (JSON.stringify(account?.owner) !== JSON.stringify(after?.owner)) throw Error('Identity changed during capture');
  return {capturedAt:new Date().toISOString(), home:await services.executionPaths.readCodexHome('local'),
    current:{globals,atoms:Object.fromEntries(core.ATOM_KEYS.filter(key=>Object.hasOwn(atoms || {},key)).map(key=>[key,atoms[key]]))},
    previousProjects, projectRemovalSupported:typeof services.projects.removeLocal === 'function',
    continuity:Object.fromEntries(Object.entries(window.__codexLocalProjectContinuity || {}).filter(([,value])=>typeof value!=='function'))};
})()
