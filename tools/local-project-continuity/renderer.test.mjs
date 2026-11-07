import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import vm from 'node:vm';
import crypto from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';
import backend from './backend.mjs';

const root=new URL('.',import.meta.url);
const project=id=>({id,name:id,rootPaths:[`D:/test/${id}`],createdAt:1,updatedAt:2});
const assignment=id=>({projectKind:'local',projectId:id});
const waitFor=async predicate=>{
  const deadline=Date.now()+5000;
  while(!predicate()) {
    if(Date.now()>deadline) throw Error('Renderer did not settle');
    await new Promise(resolve=>setTimeout(resolve,10));
  }
};

test('full renderer restores new identities and page boots, retains subsequent edits and deletion',async()=>{
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'codex-continuity-renderer-'));
  const home=path.join(directory,'home'),dataDir=path.join(directory,'data');
  fs.mkdirSync(home);
  const db=new DatabaseSync(path.join(home,'state_5.sqlite'));
  db.exec("CREATE TABLE threads(id TEXT PRIMARY KEY); INSERT INTO threads VALUES ('t1'),('t2');");
  db.close();
  let state={'electron-account-renderer-state-v1':{owner:null,inactive:{ownerA:{
    globals:{'local-projects':{p1:project('p1')},'project-order':['p1'],'thread-project-assignments':{t1:assignment('p1')}},
    atoms:{last_completed_onboarding:42,'thread-workspace-state-v1:t1':{
      revision:'research-path',project:assignment('p1'),applied:null,
      pending:{cwd:'D:/test/p1/module',projectSources:['D:/test/p1'],runtimeWorkspaceRoots:['D:/test/p1']}}},
  }}}};
  const flush=()=>fs.writeFileSync(path.join(home,'.codex-global-state.json'),JSON.stringify(state));
  flush();
  const services={
    projects:{getLocalProjects:async()=>structuredClone(state['local-projects'] || {}),setProjectOrder:async value=>{state['project-order']=value;flush();},
      removeLocal:async id=>{
        delete state['local-projects'][id];
        state['project-order']=(state['project-order'] || []).filter(value=>value!==id);
        for(const [threadId,value] of Object.entries(state['thread-project-assignments'] || {})) {
          if(value.projectId===id) {
            delete state['thread-project-assignments'][threadId];
            state['projectless-thread-ids']=[...(state['projectless-thread-ids'] || []),threadId];
          }
        }
        flush();
      }},
    executionPaths:{readCodexHome:async()=>home},
    applicationState:{read:async key=>({value:state[key]??null}),write:async(key,value)=>{
      assert.ok(!['thread-project-assignments','project-order','projectless-thread-ids'].includes(key));
      state[key]=value;flush();return {success:true};
    }},
    threadProjectAssignments:{assignIfUnassigned:async entries=>{
      state['thread-project-assignments'] ||= {};
      for(const entry of entries) state['thread-project-assignments'][entry.threadId] ||= entry.assignment;
      flush();
    }},
  };
  globalThis.__localContinuityTestFixtures ||= new Map();
  async function startRenderer() {
    const id=crypto.randomUUID();
    globalThis.__localContinuityTestFixtures.set(id,services);
    const api=backend({dataDir});
    const disposals=[];
    const context=vm.createContext({console,crypto,setTimeout,clearTimeout,structuredClone,
      document:{querySelectorAll:()=>[{src:'app://-/assets/native-runtime.js'}]},
      window:{codexPlus:{extension:apply=>apply({
        disposed:false,backend:{call:async(method,parameters)=>api[method](parameters)},
        registerMenuItem(){},setInterval(){},listen(){},toast(){},on(event,callback){if(event==='dispose')disposals.push(callback);},
      })}},
    });
    context.codexPlus=context.window.codexPlus;
    context.window=context;
    for(const name of ['core.js','renderer.js']) {
      const source=fs.readFileSync(new URL(name,root),'utf8');
      const script=new vm.Script(source,{filename:name,
        importModuleDynamically:()=>import(new URL(`test-native-runtime.mjs?fixture=${id}`,root))});
      script.runInContext(context);
    }
    const status=context.window.__codexLocalProjectContinuity;
    await waitFor(()=>status.phase==='ready');
    return {status,dispose:()=>disposals.forEach(fn=>fn())};
  }
  let renderer=await startRenderer();
  assert.equal(renderer.status.projects,1);
  assert.equal(state['thread-project-assignments'].t1.projectId,'p1');
  assert.equal(state['electron-persisted-atom-state']['electron:onboarding-welcome-pending'],false);
  // A second subscription starts with the same account-scoped globals cleared.
  state={'electron-account-renderer-state-v1':{owner:'ownerB',inactive:{}}};flush();
  await renderer.status.check();
  assert.equal(renderer.status.projects,1);
  assert.equal(state['thread-project-assignments'].t1.projectId,'p1');
  // User edits and reclassification must become the next shared baseline.
  assert.equal(state['electron-persisted-atom-state']['thread-workspace-state-v1:t1'].pending.cwd,'D:/test/p1/module');
  state['local-projects'].p2=project('p2');
  state['project-order']=['p2','p1'];
  state['thread-project-assignments'].t1=assignment('p2');
  state['thread-project-assignments'].t2=assignment('p2');flush();
  await renderer.status.check();
  state={'electron-account-renderer-state-v1':{owner:'ownerA',inactive:{}}};flush();
  await renderer.status.check();
  assert.equal(renderer.status.projects,2);
  assert.equal(state['thread-project-assignments'].t1.projectId,'p2');
  assert.deepEqual([...state['project-order']],['p2','p1']);
  // Deliberate removal is retained after another complete renderer boot.
  delete state['local-projects'].p1;state['project-order']=['p2'];flush();
  await renderer.status.check();
  renderer.dispose();
  // The former subscription still has the removed project in its cached list.
  state={'electron-account-renderer-state-v1':{owner:null,inactive:{}},
    'local-projects':{p1:project('p1'),p2:project('p2')},'project-order':['p1','p2'],
    'thread-project-assignments':{t1:assignment('p2'),t2:assignment('p2')}};flush();
  renderer=await startRenderer();
  assert.deepEqual(Object.keys(state['local-projects']),['p2']);
  assert.equal(renderer.status.assignments,2);
  assert.equal(renderer.status.removedProjects,1);
  // A newly added project is accepted; the former project is still suppressed.
  state={'electron-account-renderer-state-v1':{owner:'ownerC',inactive:{}},
    'local-projects':{p1:project('p1'),p3:project('p3')},'project-order':['p1','p3']};flush();
  await renderer.status.check();
  assert.deepEqual(Object.keys(state['local-projects']).sort(),['p2','p3']);
  assert.equal(renderer.status.assignments,2);
  // Re-adding an entry explicitly during normal operation remains supported.
  state['local-projects'].p1=project('p1');flush();
  await renderer.status.check();
  state={'electron-account-renderer-state-v1':{owner:'ownerD',inactive:{}}};flush();
  await renderer.status.check();
  assert.deepEqual(Object.keys(state['local-projects']).sort(),['p1','p2','p3']);
  renderer.dispose();
});
