import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import core from '../tools/local-project-continuity/core.js';

const [directory, originalDirectory=directory]=process.argv.slice(2);
if(!directory || !path.isAbsolute(directory)) throw Error('Provide the explicit snapshot directory');
const decode=file=>JSON.parse(fs.readFileSync(file,'utf8'));
const hash=value=>crypto.createHash('sha256').update(value).digest('hex');
const baseline=decode(path.join(directory,'native-before.json'));
const home=fs.realpathSync(baseline.home);
const homeHash=hash(process.platform==='win32'?home.toLowerCase():home);
const configRoot=path.join(process.env.APPDATA,'Codex++');
const dataDirectory=path.join(configRoot,'extension-data','local-project-continuity',homeHash);
const snapshot=decode(path.join(dataDirectory,'shared-projects.json'));
const pointer=path.join(process.env.USERPROFILE,'settings.path');
const settingsFile=fs.existsSync(pointer)?fs.readFileSync(pointer,'utf8').trim():path.join(process.env.USERPROFILE,'.codex-session-delete','settings.json');
const settings=decode(settingsFile);
const extensionConfig=decode(path.join(configRoot,'user_scripts.json'));
assert.equal(extensionConfig.enabled,true);
assert.equal(extensionConfig.scripts['user:local-project-continuity'],true);
const profiles=settings.relayProfiles.map(profile=>({id:profile.id,name:profile.name,
  active:profile.id===settings.activeRelayId,useCommonConfig:profile.useCommonConfig,
  projectStateOverrideMention:/local-projects|shared-projects|\.codex-global-state|extension-data/i.test(profile.configContents || '')}));
const scopes=[{label:'current-native',current:baseline.current}];
for(const name of fs.readdirSync(path.join(dataDirectory,'backups')).filter(name=>name.endsWith('.json'))) {
  const backup=decode(path.join(dataDirectory,'backups',name));
  if(backup.nativeBefore) scopes.push({label:`backup:${name}`,current:backup.nativeBefore});
  if(backup.state) scopes.push({label:`state:${name}`,current:{globals:backup.state,atoms:backup.state['electron-persisted-atom-state'] || {}}});
  for(const [owner,scope] of Object.entries(backup.state?.['electron-account-renderer-state-v1']?.inactive || {})) {
    scopes.push({label:`inactive:${name}:${hash(owner).slice(0,8)}`,current:scope});
  }
}
const removed=new Set(Object.keys(snapshot.removedProjects || {}));
const replays=[];
for(const profile of profiles) for(const scope of scopes) {
  const plan=core.plan(snapshot,scope.current);
  const planned=plan.updates['local-projects'] || scope.current.globals?.['local-projects'] || {};
  assert.ok(Object.keys(planned).every(id=>!removed.has(id)),'An intentionally removed project was planned for restoration');
  assert.ok(Object.keys(snapshot.globals['local-projects']).every(id=>Object.hasOwn(planned,id)),'A retained project was lost');
  assert.ok(plan.removedProjectIds.every(id=>removed.has(id)),'A retained project was planned for removal');
  replays.push({profileId:profile.id,cache:scope.label,projectCount:plan.projectCount,
    staleEntriesSuppressed:plan.removedProjectIds.length,passed:true});
}
const original=decode(path.join(originalDirectory,'snapshot-manifest.json'));
const database=new DatabaseSync(path.join(home,'state_5.sqlite'),{readOnly:true});
let threads;
try {threads=database.prepare('SELECT id,archived FROM threads ORDER BY id').all();} finally {database.close();}
assert.equal(threads.length,original.threadCount,'Thread count changed');
assert.equal(hash(JSON.stringify(threads)),original.threadIdsAndArchiveHash,'Thread IDs or archive states changed');
const installed={};
for(const name of ['core.js','renderer.js','backend.mjs','codexpp.json']) {
  const source=fs.readFileSync(new URL('../tools/local-project-continuity/'+name,import.meta.url));
  const target=fs.readFileSync(path.join(configRoot,'user_scripts','local-project-continuity',name));
  assert.equal(hash(source),hash(target),'Installed extension differs from tested source');
  installed[name]=hash(source);
}
const report={checkedAt:new Date().toISOString(),settingsFile,settingsSha256:hash(fs.readFileSync(settingsFile)),
  profiles,profileCount:profiles.length,aggregateProfileCount:(settings.aggregateRelayProfiles || []).length,
  providerSyncEnabled:settings.providerSyncEnabled,sessionIndexRepairEnabled:settings.sessionIndexRepairEnabled,
  allProfilesUseCommonConfig:profiles.every(profile=>profile.useCommonConfig===true),
  noProfileProjectStateOverride:profiles.every(profile=>!profile.projectStateOverrideMention),
  commonHome:home,extensionEnabled:true,extensionVersion:'0.2.0',snapshotVersion:snapshot.version,
  snapshotRevision:snapshot.revision,currentProjectCount:Object.keys(snapshot.globals['local-projects']).length,
  assignmentCount:Object.keys(snapshot.globals['thread-project-assignments']).length,removedProjectCount:removed.size,
  threadCount:threads.length,threadIdsAndArchiveStatesUnchanged:true,installed,replayCases:replays.length,replays,
  actualSubscriptionsSwitched:false,scope:'Read-only subscription settings audit and cached-state replay; no login/provider change or app restart'};
fs.writeFileSync(path.join(directory,'subscriptions-audit.json'),JSON.stringify(report,null,2)+'\n');
fs.copyFileSync(path.join(dataDirectory,'shared-projects.json'),path.join(directory,'continuity-after.json'));
const manifestFile=path.join(directory,'snapshot-manifest.json');
const manifest=decode(manifestFile);
manifest.files={};
for(const name of fs.readdirSync(directory)) {
  const file=path.join(directory,name);
  if(fs.statSync(file).isFile() && name!=='snapshot-manifest.json') manifest.files[name]=hash(fs.readFileSync(file));
}
fs.writeFileSync(manifestFile,JSON.stringify(manifest,null,2)+'\n');
console.log(JSON.stringify({...report,replays:undefined,installed:undefined,settingsSha256:undefined},null,2));
