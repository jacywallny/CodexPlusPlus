import fs from 'node:fs';
import core from '../tools/local-project-continuity/core.js';
import { existingThreads } from '../tools/local-project-continuity/backend.mjs';
const [home] = process.argv.slice(2);
if (!home) throw Error('Pass the native Codex home');
const state=JSON.parse(fs.readFileSync(`${home}/.codex-global-state.json`,'utf8'));
const valid=existingThreads(home);
const snapshot=core.seed(state,valid);
console.log(JSON.stringify({existingDatabaseThreads:valid.length,eligibleLocalProjects:Object.keys(snapshot.globals['local-projects']).length,
  eligibleAssignments:Object.keys(snapshot.globals['thread-project-assignments']).length,
  onboardingCompletionAvailable:Object.hasOwn(snapshot.atoms,'last_completed_onboarding'),
  currentProjects:Object.keys(state['local-projects'] || {}).length},null,2));
