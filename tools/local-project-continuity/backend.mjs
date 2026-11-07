import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';
import core from './core.js';

const hash = text => crypto.createHash('sha256').update(text).digest('hex');
const decode = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const same = (left, right) => JSON.stringify(left) === JSON.stringify(right);
const BACKUP_FILE = /^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}-\d{3}Z-[0-9a-f-]+\.json$/i;
const BACKUP_KEEP = 3;
function atomic(file, value) {
  fs.mkdirSync(path.dirname(file), {recursive:true});
  const temporary = `${file}.tmp-${crypto.randomUUID()}`;
  const descriptor = fs.openSync(temporary, 'wx');
  try { fs.writeFileSync(descriptor, JSON.stringify(value, null, 2)); fs.fsyncSync(descriptor); }
  finally { fs.closeSync(descriptor); }
  fs.renameSync(temporary, file);
}

function pruneBackups(directory) {
  if (!fs.existsSync(directory)) return [];
  const backups = fs.readdirSync(directory, {withFileTypes:true})
    .filter(entry => entry.isFile() && BACKUP_FILE.test(entry.name))
    .map(entry => {
      const file = path.join(directory, entry.name);
      return {file, name:entry.name, modified:fs.statSync(file).mtimeMs};
    })
    .sort((left, right) => right.modified - left.modified || right.name.localeCompare(left.name));
  const removed = [];
  for (const entry of backups.slice(BACKUP_KEEP)) {
    fs.unlinkSync(entry.file);
    removed.push(entry.file);
  }
  return removed;
}

export function existingThreads(home) {
  const names = fs.readdirSync(home).filter(name => /^state_\d+\.sqlite$/.test(name))
    .sort((a,b) => Number(b.match(/\d+/)[0]) - Number(a.match(/\d+/)[0]));
  if (!names.length) throw Error('No Codex thread database found; refusing unverified assignments');
  const database = new DatabaseSync(path.join(home, names[0]), {readOnly:true});
  try {
    return database.prepare('SELECT id FROM threads').all().map(row => row.id);
  } finally { database.close(); }
}

export default function ({dataDir}) {
  const homes = new Map();
  function location(home) {
    if (typeof home !== 'string' || !path.isAbsolute(home)) throw Error('A native absolute Codex home is required');
    const real = fs.realpathSync(home);
    const identity = hash(process.platform === 'win32' ? real.toLowerCase() : real);
    return {home:real, directory:path.join(dataDir, identity)};
  }
  function acquire(home, windowId) {
    const target = location(home);
    let record = homes.get(target.home);
    if (record?.windowId !== windowId && record?.expires > Date.now()) return null;
    if (!record || record.windowId !== windowId) record = {...target, windowId, token:crypto.randomUUID()};
    record.expires = Date.now() + 20000;
    homes.set(target.home, record);
    return record;
  }
  function requireLease({home, token}) {
    const record = homes.get(location(home).home);
    if (!record || record.token !== token || record.expires <= Date.now()) throw Error('Continuity lease expired');
    record.expires = Date.now() + 20000;
    return record;
  }
  function currentSnapshot(record) {
    const file = path.join(record.directory, 'shared-projects.json');
    if (fs.existsSync(file)) return decode(file);
    const raw = decode(path.join(record.home, '.codex-global-state.json'));
    const snapshot = {version:1, revision:0, ...core.seed(raw, existingThreads(record.home))};
    atomic(file, snapshot);
    return snapshot;
  }
  return {
    prepare({home, windowId}) {
      const record = acquire(home, windowId);
      if (!record) return {busy:true};
      const snapshot = currentSnapshot(record);
      const validThreadIds = existingThreads(record.home);
      const filtered = core.sanitize(snapshot.globals, snapshot.atoms, validThreadIds, snapshot.removedProjects);
      return {token:record.token, snapshot:{...snapshot,...filtered}, validThreadIds,
        backupDirectory:path.join(record.directory, 'backups')};
    },
    backup(parameters) {
      const record = requireLease(parameters);
      const state = decode(path.join(record.home, '.codex-global-state.json'));
      const file = path.join(record.directory, 'backups', `${new Date().toISOString().replace(/[:.]/g,'-')}-${crypto.randomUUID()}.json`);
      atomic(file, {version:1, createdAt:new Date().toISOString(), state,
        nativeBefore:parameters.before, planned:parameters.counts});
      return {file, removed:pruneBackups(path.dirname(file))};
    },
    commit(parameters) {
      const record = requireLease(parameters);
      const file = path.join(record.directory, 'shared-projects.json');
      const previous = currentSnapshot(record);
      if (previous.revision !== parameters.revision) throw Error('Snapshot changed; refusing stale commit');
      const next = core.advance(previous, parameters.current, existingThreads(record.home), parameters);
      if (same({globals:previous.globals,atoms:previous.atoms,removedProjects:previous.removedProjects || {}},next)) return {revision:previous.revision, changed:false};
      atomic(file, {version:2, revision:previous.revision+1, updatedAt:new Date().toISOString(), ...next});
      return {revision:previous.revision+1, changed:true};
    },
  };
}
