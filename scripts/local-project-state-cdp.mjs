import { readFile } from 'node:fs/promises';

const [expressionPath, port = '9229'] = process.argv.slice(2);
if (!expressionPath) throw Error('Usage: node scripts/local-project-state-cdp.mjs expression.js [port]');
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const pages = targets.filter(target => target.type === 'page' && target.url === 'app://-/index.html');
if (pages.length !== 1) throw Error(`Expected one Codex page, found ${pages.length}`);
const expression = await readFile(expressionPath, 'utf8');
const socket = new WebSocket(pages[0].webSocketDebuggerUrl);
const response = await new Promise((resolve, reject) => {
  const timeout = setTimeout(() => reject(Error('CDP evaluation timed out')), 60000);
  socket.addEventListener('error', reject, { once: true });
  socket.addEventListener('open', () => socket.send(JSON.stringify({
    id: 1, method: 'Runtime.evaluate', params: { expression, awaitPromise: true, returnByValue: true },
  })), { once: true });
  socket.addEventListener('message', ({ data }) => {
    const result = JSON.parse(data);
    if (result.id !== 1) return;
    clearTimeout(timeout);
    resolve(result);
  });
});
socket.close();
if (response.error || response.result?.exceptionDetails) {
  console.error(JSON.stringify(response.error || response.result.exceptionDetails, null, 2));
  process.exitCode = 1;
} else {
  console.log(JSON.stringify(response.result.result.value, null, 2));
}
