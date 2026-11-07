(() => {
  'use strict';
  const core = window.__CodexLocalProjectContinuityCore;
  if (!core || !window.codexPlus?.extension) throw Error('Codex++ extension API is unavailable');
  window.codexPlus.extension(ctx => {
    const status = {version:'0.2.1', phase:'starting', projects:0, assignments:0, removedProjects:0, error:null};
    window.__codexLocalProjectContinuity = status;
    let services, home, token, revision, prepared, busy=false, generation=null, ready=false;
    let stopped = false;
    const windowId = crypto.randomUUID();
    const within = (promise, timeout=12000) => new Promise((resolve,reject) => {
      const timer=setTimeout(()=>reject(Error('Native continuity operation timed out')),timeout);
      Promise.resolve(promise).then(value=>{clearTimeout(timer);resolve(value);},error=>{clearTimeout(timer);reject(error);});
    });
    const read = async key => {
      const result = await within(services.applicationState.read(key));
      if (!result || !Object.hasOwn(result,'value')) throw Error(`Native state read rejected: ${key}`);
      return result.value;
    };
    const identity = async () => {
      const state = await read('electron-account-renderer-state-v1');
      if (!state || !Object.hasOwn(state,'owner') || !state.inactive) throw Error('Unsupported account state structure');
      return JSON.stringify(state.owner);
    };
    async function assertIdentity() {
      if (stopped || ctx.disposed || await identity() !== generation) throw Error('Identity changed during continuity repair; retrying');
    }
    async function write(key,value) {
      await assertIdentity();
      const result = await within(services.applicationState.write(key,value));
      if (result?.success !== true) throw Error(`Native state write rejected: ${key}`);
    }
    async function discover() {
      const candidates = new Set();
      for (const script of document.querySelectorAll('script[src]')) {
        if (!script.src.startsWith('app://-/assets/')) continue;
        const namespace = await import(script.src);
        const runtime = namespace.__webpack_require__;
        if (!runtime?.c) continue;
        for (const cached of Object.values(runtime.c)) {
          for (const value of Object.values(cached.exports || {})) {
            if (value && typeof value==='object' && Object.hasOwn(value,'projects')
                && Object.hasOwn(value,'applicationState') && Object.hasOwn(value,'threadProjectAssignments')
                && typeof value.projects.getLocalProjects==='function' && typeof value.applicationState.read==='function'
                && typeof value.applicationState.write==='function') candidates.add(value);
          }
        }
      }
      if (candidates.size!==1) throw Error(`Unsupported native services: ${candidates.size}`);
      services=[...candidates][0];
      home=await within(services.executionPaths.readCodexHome('local'));
    }
    async function readCurrent() {
      const values=await Promise.all(core.GLOBAL_KEYS.map(key=>read(key)));
      return {globals:Object.fromEntries(core.GLOBAL_KEYS.map((key,index)=>[key,values[index]])),
        atoms:await read('electron-persisted-atom-state') || {}};
    }
    async function restore() {
      status.phase='checking';
      let current=await readCurrent();
      let plan=core.plan(prepared.snapshot,current);
      const orderChanged=JSON.stringify(plan.order)!==JSON.stringify(current.globals['project-order'] || []);
      const changed=Object.keys(plan.updates).length || plan.assignments.length || orderChanged || plan.atomsChanged || plan.removedProjectIds.length;
      if(changed) {
        const backup=await ctx.backend.call('backup',{home,token,before:current,
          counts:{projects:plan.projectCount,assignments:plan.assignments.length,keys:Object.keys(plan.updates)}});
        status.backup=backup.file;
        for(const id of plan.removedProjectIds) {
          await assertIdentity();
          if(typeof services.projects.removeLocal!=='function') throw Error('Native project removal is unavailable');
          await within(services.projects.removeLocal(id));
        }
        if(plan.removedProjectIds.length) {
          current=await readCurrent();
          plan=core.plan(prepared.snapshot,current);
          if(plan.removedProjectIds.length) throw Error('Native project readback still contains removed entries');
        }
        for(const [key,value] of Object.entries(plan.updates)) await write(key,value);
        if(orderChanged) { await assertIdentity(); await within(services.projects.setProjectOrder(plan.order)); }
        for(let index=0;index<plan.assignments.length;index+=100) {
          await assertIdentity();
          await within(services.threadProjectAssignments.assignIfUnassigned(plan.assignments.slice(index,index+100)));
        }
        if(plan.atomsChanged) await write('electron-persisted-atom-state',plan.atoms);
      }
      await assertIdentity();
      const actual=await readCurrent();
      const projects=await within(services.projects.getLocalProjects());
      for(const id of Object.keys(prepared.snapshot.globals['local-projects'])) {
        if(!Object.hasOwn(projects,id)) throw Error('Native project readback did not confirm restoration');
      }
      const actualAssignments=actual.globals['thread-project-assignments'] || {};
      for(const entry of plan.assignments) {
        if(!Object.hasOwn(actualAssignments,entry.threadId)) throw Error('Native assignment readback did not confirm restoration');
      }
      status.projects=Object.keys(projects).length;
      status.assignments=Object.keys(actualAssignments).length;
      status.restoredAssignments=plan.assignments.length;
      await ctx.backend.call('commit',{home,token,revision,current:actual,mode:'restore'});
      ready=true;
      status.phase='ready';status.error=null;status.verifiedAt=new Date().toISOString();
      if(changed) ctx.toast(`本地项目已恢复：${status.projects} 个项目；切换后将自动检查`,{type:'success'});
    }
    async function tick() {
      if(busy || stopped || ctx.disposed) return;
      busy=true;
      try {
        if(!services) await discover();
        const currentGeneration=await identity();
        if(currentGeneration!==generation) { generation=currentGeneration;ready=false; }
        prepared=await ctx.backend.call('prepare',{home,windowId});
        if(prepared.busy) {status.phase='standby';return;}
        token=prepared.token;revision=prepared.snapshot.revision;
        status.removedProjects=Object.keys(prepared.snapshot.removedProjects || {}).length;
        if(!ready) await restore();
        else {
          const current=await readCurrent();
          await assertIdentity();
          await ctx.backend.call('commit',{home,token,revision,current});
          status.projects=Object.keys(current.globals['local-projects'] || {}).length;
          status.assignments=Object.keys(current.globals['thread-project-assignments'] || {}).length;
          status.verifiedAt=new Date().toISOString();
          status.phase='ready';status.error=null;
        }
      } catch(error) {
        status.phase='retrying';status.error=String(error?.message || error);
        console.warn('[local-project-continuity]',status.error);
      } finally {busy=false;}
    }
    ctx.registerMenuItem({label:'本地项目连续性修复',description:'自动检查启动和身份切换后的项目归属',
      onActivate:()=>ctx.toast(status.error || `${status.phase}：${status.projects} 个项目，${status.assignments} 条归属`,{type:status.error?'error':'info'})});
    ctx.setInterval(tick,5000);
    ctx.listen(window,'focus',tick);
    ctx.on('dispose',()=>{stopped=true;});
    status.check=tick;
    status.rememberRemovedProjects=async removedProjects=>{
      if(busy || !ready || stopped || ctx.disposed) throw Error('Continuity is not ready to save the current baseline');
      busy=true;
      try {
        const saved=await ctx.backend.call('prepare',{home,windowId});
        if(saved.busy) throw Error('Another window owns the continuity lease');
        const current=await readCurrent();
        await assertIdentity();
        await ctx.backend.call('commit',{home,token:saved.token,revision:saved.snapshot.revision,current,removedProjects});
      } finally {busy=false;}
      await tick();
      if(status.phase!=='ready') throw Error(status.error || 'Current project baseline was not saved');
      return {projects:status.projects,assignments:status.assignments,removedProjects:status.removedProjects};
    };
    void tick();
  });
})();
