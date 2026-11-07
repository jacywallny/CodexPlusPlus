import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import core from './core.js';
import backend from './backend.mjs';

const project = (id,name=id) => ({id,name,rootPaths:[`D:/test/${id}`],createdAt:1,updatedAt:2});
const assignment = projectId => ({projectKind:'local',projectId});
const snapshot = () => ({globals:{'local-projects':{p1:project('p1')},'project-order':['p1'],
  'thread-project-assignments':{t1:assignment('p1')}},atoms:{last_completed_onboarding:1}});

const workspaceState=(cwd='D:/test/p1/module')=>({revision:'new-revision',project:assignment('p1'),applied:null,
  pending:{cwd,projectSources:['D:/test/p1'],runtimeWorkspaceRoots:['D:/test/p1']}});
const workspaceKey='thread-workspace-state-v1:t1';

test('local workspace cwd survives cleared and stale account atoms',()=>{
  const saved=snapshot();saved.atoms[workspaceKey]=workspaceState();
  const sanitized=core.sanitize(saved.globals,saved.atoms,['t1']);
  assert.deepEqual(sanitized.atoms[workspaceKey],workspaceState());
  for(const current of [{globals:{},atoms:{}},{globals:structuredClone(saved.globals),atoms:{[workspaceKey]:workspaceState('D:/test/p1')}}]) {
    const plan=core.plan(sanitized,current);
    assert.equal(plan.atoms[workspaceKey].pending.cwd,'D:/test/p1/module');
    const roundTrip=core.sanitize({...saved.globals,...plan.updates},plan.atoms,['t1']);
    assert.deepEqual(roundTrip.atoms[workspaceKey],workspaceState());
  }
});

test('workspace restore respects reassignment, projectless choices and removed projects',()=>{
  const saved=snapshot();saved.atoms[workspaceKey]=workspaceState();
  for(const globals of [
    {'thread-project-assignments':{t1:assignment('p2')},'local-projects':{p2:project('p2')}},
    {'projectless-thread-ids':['t1']},
  ]) assert.equal(core.plan(saved,{globals,atoms:{}}).atoms[workspaceKey],undefined);
  const removed=core.advance(saved,{globals:{},atoms:{}},['t1']);
  assert.equal(core.plan(removed,{globals:saved.globals,atoms:{}}).atoms[workspaceKey],undefined);
});

test('workspace snapshots reject stale IDs, outside paths, remote memberships and extra data',()=>{
  for(const value of [workspaceState('D:/outside'),{...workspaceState(),project:{projectKind:'remote',projectId:'p1'}},
    {...workspaceState(),project:{...assignment('p1'),token:'private'}},
    {...workspaceState(),auth:'private'}, {...workspaceState(),pending:{...workspaceState().pending,token:'private'}}]) {
    const saved=snapshot();saved.atoms[workspaceKey]=value;
    assert.equal(core.sanitize(saved.globals,saved.atoms,['t1']).atoms[workspaceKey],undefined);
  }
  const saved=snapshot();saved.atoms[workspaceKey]=workspaceState();
  assert.equal(core.sanitize(saved.globals,saved.atoms,[]).atoms[workspaceKey],undefined);
});

test('recovers inactive local metadata, filters stale thread IDs and cloud-backed projects',()=>{
  const state={'electron-account-renderer-state-v1':{owner:null,inactive:{ownerA:{globals:{
    'local-projects':{p1:project('p1'),cloud:{...project('cloud'),chatGptBacking:{accountId:'other'}}},
    'project-order':['p1','cloud'],'thread-project-assignments':{t1:assignment('p1'),stale:assignment('p1'),t2:assignment('cloud')},
  },atoms:{last_completed_onboarding:42,'prompt-history':['private']}}}},'electron-persisted-atom-state':{}};
  const result=core.seed(state,['t1','t2']);
  assert.deepEqual(Object.keys(result.globals['local-projects']),['p1']);
  assert.deepEqual(Object.keys(result.globals['thread-project-assignments']),['t1']);
  assert.equal(result.atoms['electron:onboarding-welcome-pending'],false);
  assert.equal(result.atoms['prompt-history'],undefined);
});

test('does not invent onboarding completion for a user who has never completed it',()=>{
  const result=core.sanitize({}, {'electron:onboarding-welcome-pending':true},[]);
  assert.deepEqual(result.atoms,{});
});

test('restore retains target projects, edits, assignments and explicit projectless choices',()=>{
  const saved=snapshot();
  saved.globals['thread-project-assignments'].t2=assignment('p1');
  saved.globals['thread-project-assignments'].t3=assignment('p1');
  const current={globals:{'local-projects':{p1:project('p1','edited'),p2:project('p2')},
    'project-order':['p2','p1'],'thread-project-assignments':{t2:assignment('p2')},'projectless-thread-ids':['t3']},atoms:{'unrelated-preference':true}};
  const plan=core.plan(saved,current);
  assert.equal(plan.projectCount,2);
  assert.deepEqual(plan.assignments.map(entry=>entry.threadId),['t1']);
  assert.deepEqual(plan.order,['p2','p1']);
  assert.equal(plan.atoms['unrelated-preference'],true);
  assert.ok(!Object.hasOwn(plan.updates,'thread-project-assignments'));
});

test('does not copy prompt, permission, auth or remote project state',()=>{
  const result=core.seed({'local-projects':{p1:project('p1')},'remote-projects':[{id:'remote'}],
    'provider-token-cache':'secret','electron-persisted-atom-state':{'heartbeat-thread-permissions-by-id':{t1:'secret'}},
    'app-server-projects-migration-by-host':{'remote:test':true,'local:D:/home':true}},[]);
  assert.equal(JSON.stringify(result).includes('secret'),false);
  assert.equal(Object.hasOwn(result.globals,'remote-projects'),false);
  assert.deepEqual(result.globals['app-server-projects-migration-by-host'],{'local:D:/home':true});
});

test('native restore planning is idempotent after the restored snapshot is read back',()=>{
  const saved=core.sanitize(snapshot().globals,snapshot().atoms,['t1']);
  const plan=core.plan(saved,{globals:structuredClone(saved.globals),atoms:structuredClone(saved.atoms)});
  assert.deepEqual(plan.updates,{});
  assert.equal(plan.assignments.length,0);
  assert.equal(plan.atomsChanged,false);
});

test('preserves server-ID keyed presentation and merges mappings inside the same local home',()=>{
  const saved=snapshot();
  saved.globals['app-server-project-id-by-legacy-project-id-by-host']={'local:D:/home':{p1:'server1'}};
  saved.globals['sidebar-project-thread-orders']={server1:{local:['t1']}};
  const sanitized=core.sanitize(saved.globals,{},['t1']);
  assert.deepEqual(sanitized.globals['sidebar-project-thread-orders'],{server1:{local:['t1']}});
  const plan=core.plan(sanitized,{globals:{'app-server-project-id-by-legacy-project-id-by-host':{'local:D:/home':{p2:'server2'}}},atoms:{}});
  assert.deepEqual(plan.updates['app-server-project-id-by-legacy-project-id-by-host'],{'local:D:/home':{p1:'server1',p2:'server2'}});
});

test('retains completed role choices when a new identity initializes empty onboarding defaults',()=>{
  const saved=snapshot();
  saved.atoms['electron:onboarding-welcome-v2-role-state']={roles:['engineering'],workMode:'coding',completedConversationalOnboarding:true};
  const plan=core.plan(saved,{globals:{},atoms:{'electron:onboarding-welcome-v2-role-state':{roles:[],workMode:null}}});
  assert.deepEqual(plan.atoms['electron:onboarding-welcome-v2-role-state'].roles,['engineering']);
  assert.equal(plan.atoms['electron:onboarding-welcome-v2-role-state'].workMode,'coding');
});

test('backend preserves intentional deletion and reassignment across restart and leases windows',()=>{
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'codex-project-continuity-test-'));
  const home=path.join(temp,'home'), dataDir=path.join(temp,'data');
  fs.mkdirSync(home);
  const db=new DatabaseSync(path.join(home,'state_5.sqlite'));
  db.exec('CREATE TABLE threads(id TEXT PRIMARY KEY); INSERT INTO threads VALUES (\'t1\');');
  db.close();
  fs.writeFileSync(path.join(home,'.codex-global-state.json'),JSON.stringify(snapshot().globals));
  const api=backend({dataDir});
  const prepared=api.prepare({home,windowId:'window1'});
  assert.equal(Object.keys(prepared.snapshot.globals['local-projects']).length,1);
  assert.deepEqual(api.prepare({home,windowId:'window2'}),{busy:true});
  const backup=api.backup({home,token:prepared.token,before:{},counts:{projects:1}});
  assert.ok(fs.existsSync(backup.file));
  assert.throws(()=>api.commit({home,token:'wrong',revision:0,current:{globals:{},atoms:{}}}),/expired/);
  api.commit({home,token:prepared.token,revision:0,current:{globals:{},atoms:{}}});
  const restarted=backend({dataDir}).prepare({home,windowId:'window3'});
  assert.equal(Object.keys(restarted.snapshot.globals['local-projects']).length,0);
  assert.throws(()=>api.commit({home,token:prepared.token,revision:0,current:{globals:{},atoms:{}}}),/stale/);
});

test('backend keeps only the three newest repair backups',()=>{
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'codex-project-continuity-backup-test-'));
  const home=path.join(temp,'home'), dataDir=path.join(temp,'data');
  fs.mkdirSync(home);
  const db=new DatabaseSync(path.join(home,'state_5.sqlite'));
  db.exec('CREATE TABLE threads(id TEXT PRIMARY KEY); INSERT INTO threads VALUES (\'t1\');');
  db.close();
  fs.writeFileSync(path.join(home,'.codex-global-state.json'),JSON.stringify(snapshot().globals));
  const api=backend({dataDir});
  const prepared=api.prepare({home,windowId:'window1'});
  const created=[];
  for(let index=0;index<5;index+=1) created.push(api.backup({home,token:prepared.token,before:{index},counts:{projects:index}}).file);
  const backupDirectory=path.dirname(created[0]);
  const remaining=fs.readdirSync(backupDirectory).filter(name=>name.endsWith('.json'));
  assert.equal(remaining.length,3);
  assert.equal(new Set(remaining).size,3);
  assert.ok(created.slice(-1).every(file=>fs.existsSync(file)));
});

test('removed projects stay removed when returning to an old account cache',()=>{
  const previous=snapshot();
  previous.globals['local-projects'].p2=project('p2');
  const current={globals:{'local-projects':{p2:project('p2')},'project-order':['p2']},atoms:{}};
  const saved=core.advance(previous,current,['t1']);
  assert.deepEqual(Object.keys(saved.removedProjects),['p1']);
  const stale={globals:{'local-projects':{p1:project('p1'),p3:project('p3')},
    'project-order':['p1','p3'],'pinned-project-ids':['p1'],
    'electron-saved-workspace-roots':['D:/test/p1','D:/test/p3'],
    'app-server-project-id-by-legacy-project-id-by-host':{'local:D:/home':{p1:'server1'}},
    'sidebar-project-thread-orders':{server1:{local:['t1']}}},atoms:{}};
  const plan=core.plan(saved,stale);
  assert.deepEqual(plan.removedProjectIds,['p1']);
  assert.deepEqual(Object.keys(plan.updates['local-projects']).sort(),['p2','p3']);
  assert.ok(!plan.order.includes('p1'));
  assert.ok(!plan.updates['electron-saved-workspace-roots'].includes('D:/test/p1'));
  assert.deepEqual(plan.updates['sidebar-project-thread-orders'],{});
  assert.deepEqual(plan.updates['app-server-project-id-by-legacy-project-id-by-host'],{'local:D:/home':{}});
});

test('explicit live re-add clears a marker while a restore cannot clear it',()=>{
  const previous={...snapshot(),removedProjects:{p2:project('p2')}};
  const current={globals:{'local-projects':{p1:project('p1'),p2:project('p2')}},atoms:{}};
  const restored=core.advance(previous,current,['t1'],{mode:'restore'});
  assert.deepEqual(Object.keys(restored.globals['local-projects']),['p1']);
  assert.ok(restored.removedProjects.p2);
  const readded=core.advance(previous,current,['t1']);
  assert.deepEqual(readded.removedProjects,{});
  assert.deepEqual(Object.keys(readded.globals['local-projects']),['p1','p2']);
});
