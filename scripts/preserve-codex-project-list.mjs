import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';

const [directory] = process.argv.slice(2);
if (!directory || !path.isAbsolute(directory)) throw Error('Provide an explicit snapshot directory');
const decode = file => JSON.parse(fs.readFileSync(file,'utf8'));
const write = (name,value) => fs.writeFileSync(path.join(directory,name),JSON.stringify(value,null,2)+'\n');
const native=decode(path.join(directory,'native-before.json'));
const api=decode(path.join(directory,'projects-from-app.json'));
const projects=native.current.globals['local-projects'];
const ids=Object.keys(projects).sort();
if(JSON.stringify(ids)!==JSON.stringify(api.projects.map(project=>project.projectId).sort())) throw Error('API/native project lists differ');
const hash=value=>crypto.createHash('sha256').update(value).digest('hex');
const home=fs.realpathSync(native.home);
const homeHash=hash(process.platform==='win32'?home.toLowerCase():home);
const continuityDirectory=path.join(process.env.APPDATA,'Codex++','extension-data','local-project-continuity',homeHash);
const shared=path.join(continuityDirectory,'shared-projects.json');
const beforeFile=path.join(directory,'continuity-before.json');
if(!fs.existsSync(beforeFile)) fs.copyFileSync(shared,beforeFile,fs.constants.COPYFILE_EXCL);
const previousProjects={...native.previousProjects};
const collect=state=>{
  for(const scope of Object.values(state?.['electron-account-renderer-state-v1']?.inactive || {})) Object.assign(previousProjects,scope.globals?.['local-projects']);
  Object.assign(previousProjects,state?.['local-projects']);
};
const backups=path.join(continuityDirectory,'backups');
for(const name of fs.readdirSync(backups).filter(name=>name.endsWith('.json'))) {
  const previous=decode(path.join(backups,name));
  collect(previous.state);
  Object.assign(previousProjects,previous.nativeBefore?.globals?.['local-projects']);
}
const removedProjects=Object.fromEntries(Object.entries(previousProjects).filter(([id,project])=>!Object.hasOwn(projects,id)
  && project.id===id && Array.isArray(project.rootPaths) && !project.chatGptBacking && !project.chatGptBackingRecoveryPending));
write('known-removed-projects.json',removedProjects);
fs.writeFileSync(path.join(directory,'adopt-current-baseline.js'),`(async()=>{\nconst removedProjects=${JSON.stringify(removedProjects)};\nconst deadline=Date.now()+15000;\nwhile(Date.now()<deadline){\nconst status=window.__codexLocalProjectContinuity;\nif(status?.version==='0.2.0' && status.phase==='ready' && typeof status.rememberRemovedProjects==='function'){\ntry{return await status.rememberRemovedProjects(removedProjects);}catch(error){if(!String(error.message).includes('not ready'))throw error;}\n}\nawait new Promise(resolve=>setTimeout(resolve,250));\n}\nthrow Error('Updated continuity extension did not become ready');\n})()\n`);
const assignments=native.current.globals['thread-project-assignments'] || {};
const escape=value=>'"'+String(value ?? '').replaceAll('"','""')+'"';
const projectRows=api.projects.map((project,index)=>({order:index+1,...project,
  rootPaths:projects[project.projectId].rootPaths,
  assignedThreads:Object.values(assignments).filter(value=>value.projectId===project.projectId).length}));
write('remaining-projects.json',{capturedAt:native.capturedAt,projects:projectRows,removedProjects});
const projectCsv=[['顺序','项目名称','项目ID','项目路径','所属会话数'],...projectRows.map(project=>[
  project.order,project.label,project.projectId,project.rootPaths.join(' | '),project.assignedThreads])];
fs.writeFileSync(path.join(directory,'剩余项目列表.csv'),'\uFEFF'+projectCsv.map(row=>row.map(escape).join(',')).join('\r\n'));
const membershipCsv=[['会话ID','项目ID','项目名称'],...Object.entries(assignments).map(([id,value])=>[id,value.projectId,projects[value.projectId]?.name || ''])];
fs.writeFileSync(path.join(directory,'项目会话归属.csv'),'\uFEFF'+membershipCsv.map(row=>row.map(escape).join(',')).join('\r\n'));
const database=new DatabaseSync(path.join(home,'state_5.sqlite'),{readOnly:true});
let threads;
try {threads=database.prepare('SELECT id,archived FROM threads ORDER BY id').all();} finally {database.close();}
const manifest={capturedAt:native.capturedAt,projectCount:projectRows.length,assignmentCount:Object.keys(assignments).length,
  knownRemovedCount:Object.keys(removedProjects).length,threadCount:threads.length,threadIdsAndArchiveHash:hash(JSON.stringify(threads)),files:{}};
const markdown=['# Codex 当前剩余项目列表','',`采样：${native.capturedAt}；本机 ${projectRows.length} 个项目，${Object.keys(assignments).length} 条项目归属。`,
  '','用户只是从侧栏移除项目入口。本快照不删除项目目录、文件或会话；不将历史缓存中的已移除入口重新加入。',
  '','| 顺序 | 项目 | 路径 | 会话数 |','| --- | --- | --- | ---: |',
  ...projectRows.map(project=>`| ${project.order} | ${project.label} | ${project.rootPaths.map(root=>'`'+root+'`').join('；')} | ${project.assignedThreads} |`),
  '',`从既有本机缓存/修复备份识别 ${Object.keys(removedProjects).length} 个不在当前列表中的历史入口，作为此次明确保留当前列表的移除标记。`,
  '','JSON 保留项目 ID、顺序和多根路径；CSV 便于查阅。项目会话归属 CSV 只保存会话 ID 与分类，不包含会话正文。',
  '','`continuity-before.json` 是修改前恢复快照；只供审计/按字段恢复，不应直接覆盖运行中的应用状态。',
  '','订阅切换防回填以拓展中的持久移除标记实现；允许后续新建项目及在正常运行时主动重新添加。',''];
fs.writeFileSync(path.join(directory,'剩余项目列表.md'),markdown.join('\n'));
for(const name of fs.readdirSync(directory)) {
  const file=path.join(directory,name);
  if(fs.statSync(file).isFile() && name!=='snapshot-manifest.json') manifest.files[name]=hash(fs.readFileSync(file));
}
write('snapshot-manifest.json',manifest);
console.log(JSON.stringify({...manifest,files:Object.keys(manifest.files)},null,2));
