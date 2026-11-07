(async()=>{
  if(typeof window.__codexSessionDeleteBridge!=='function') throw Error('Codex++ bridge unavailable');
  const result=await window.__codexSessionDeleteBridge('/user-scripts/reload',{});
  return {status:result.status ?? null,mode:result.reload_mode ?? null,
    extension:result.scripts?.find(script=>script.key==='user:local-project-continuity')?.package?.version ?? null};
})()
