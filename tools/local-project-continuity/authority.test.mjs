import test from 'node:test';
import assert from 'node:assert/strict';
import core from './core.js';

const project=(id,name=id,root=`D:/research/${id}`)=>({id,name,rootPaths:[root],createdAt:1,updatedAt:2});
const local=id=>({projectKind:'local',projectId:id});
const workspace=(projectId,cwd)=>({revision:'latest',project:local(projectId),applied:null,
  pending:{cwd,projectSources:['D:/research/current'],runtimeWorkspaceRoots:['D:/research/current']}});
const baseline=()=>({globals:{'local-projects':{p:project('p','current','D:/research/current'),q:project('q')},
  'project-order':['p','q'],'pinned-project-ids':['p'],'project-appearances':{p:{collapsed:true}},
  'sidebar-project-thread-orders':{p:{local:['t2','t1']}},
  'thread-project-assignments':{t1:local('p'),t2:local('p')},'projectless-thread-ids':['t3']},
  atoms:{'thread-workspace-state-v1:t1':workspace('p','D:/research/current/circuit')}});

test('old subscription cannot revert known project metadata, ordering, pins or thread placements',()=>{
  const saved=baseline();
  const stale={globals:{'local-projects':{p:project('p','old','D:/research/old'),q:project('q'),new:project('new')},
    'project-order':['q','p','new'],'pinned-project-ids':['q'],
    'project-appearances':{p:{collapsed:false},q:{collapsed:true}},
    'sidebar-project-thread-orders':{p:{local:['t1','t2']},q:{local:['old']}},
    'thread-project-assignments':{t1:local('q'),t3:local('q')},'projectless-thread-ids':['t2']},atoms:{}};
  const plan=core.plan(saved,stale);
  assert.equal(plan.updates['local-projects'].p.name,'current');
  assert.deepEqual(plan.updates['local-projects'].p.rootPaths,['D:/research/current']);
  assert.deepEqual(plan.order,['p','q','new']);
  assert.deepEqual(plan.updates['pinned-project-ids'],['p']);
  assert.deepEqual(plan.updates['project-appearances'],{p:{collapsed:true}});
  assert.deepEqual(plan.updates['sidebar-project-thread-orders'],{p:{local:['t2','t1']}});
  assert.deepEqual(plan.membershipCorrections.map(entry=>[entry.threadId,entry.assignment,entry.projectless]),[
    ['t1',local('p'),false],['t2',local('p'),false],['t3',null,true]]);
  assert.equal(plan.atoms['thread-workspace-state-v1:t1'].pending.cwd,'D:/research/current/circuit');
});

test('normal latest edits become authoritative on the subsequent subscription restore',()=>{
  const old=baseline();
  const current=structuredClone(old);
  current.globals['local-projects'].p.name='latest edit';
  current.globals['project-order']=['q','p'];
  current.globals['pinned-project-ids']=[];
  current.globals['thread-project-assignments'].t1=local('q');
  current.atoms={};
  const latest=core.advance(old,current,['t1','t2','t3']);
  const plan=core.plan(latest,old);
  assert.equal(plan.updates['local-projects'].p.name,'latest edit');
  assert.deepEqual(plan.order,['q','p']);
  assert.deepEqual(plan.updates['pinned-project-ids'],[]);
  assert.equal(plan.membershipCorrections.find(entry=>entry.threadId==='t1').assignment.projectId,'q');
});

test('removing an old source project cannot strand the moved chat in projectless state',()=>{
  const saved=baseline();saved.removedProjects={old:project('old')};
  const afterNativeRemoval={globals:{'local-projects':{},'projectless-thread-ids':['t1']},atoms:{}};
  const plan=core.plan(saved,afterNativeRemoval);
  assert.deepEqual(plan.membershipCorrections.find(entry=>entry.threadId==='t1').assignment,local('p'));
  assert.equal(plan.updates['local-projects'].old,undefined);
});

test('restore leaves remote memberships and unknown local projects outside the saved baseline intact',()=>{
  const saved=baseline();
  const remote={projectKind:'remote',projectId:'remote',hostId:'ssh'};
  const current={globals:{'local-projects':{new:project('new')},
    'thread-project-assignments':{t1:remote,other:local('new')}},atoms:{}};
  const plan=core.plan(saved,current);
  assert.equal(plan.membershipCorrections.some(entry=>entry.threadId==='t1'),false);
  assert.equal(plan.membershipCorrections.some(entry=>entry.threadId==='other'),false);
  assert.deepEqual(plan.updates['local-projects'].new,project('new'));
  assert.equal(plan.atoms['thread-workspace-state-v1:t1'],undefined);
});
