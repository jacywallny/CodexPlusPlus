import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

const [executableArg, scratchArg] = process.argv.slice(2);
assert.ok(executableArg && scratchArg, "pass the Windows gateway executable and an isolated scratch directory");
const executable = path.resolve(executableArg);
const root = fs.mkdtempSync(path.join(path.resolve(scratchArg), "stream-health-293-"));
const state = path.join(root, "state");
const codexHome = path.join(root, "codex");
for (const directory of [state, codexHome]) fs.mkdirSync(directory);
const sol = "gpt-6.2-sol";
const astra = "gpt-6.2-astra";
const counts = new Map();
const reachedRecovery = Promise.withResolvers();
const completeRecovery = Promise.withResolvers();
let failSol = true;
let heldRecovery = false;
let upstreamError;

function bump(provider, model, fixture) {
  const key = `${provider}/${model}/${fixture}`;
  const next = (counts.get(key) ?? 0) + 1;
  counts.set(key, next);
  return next;
}

function createUpstream(provider) {
  const server = http.createServer(async (request, response) => {
    try {
      if (request.method === "GET" && request.url === "/v1/models") {
        response.setHeader("content-type", "application/json");
        return response.end(JSON.stringify({ data: [sol, astra].map(id => ({ id, object: "model" })) }));
      }
      assert.equal(request.url, "/v1/responses");
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      const body = JSON.parse(Buffer.concat(chunks));
      const model = body.model;
      const fixture = body.input[0].content[0].text;
      const attempt = bump(provider, model, fixture);
      assert.equal(attempt, 1, "a fixture request was replayed");
      if (fixture.startsWith("route-budget-")) {
        await sleep(1_600);
        response.destroy();
        return;
      }
      response.writeHead(200, { "content-type": "text/event-stream" });
      response.flushHeaders();
      const send = event => response.write("data: " + JSON.stringify(event) + "\n\n");
      const delta = { type: "response.output_text.delta", delta: "fixture output" };
      const done = { type: "response.completed", response: { status: "completed", output: [] } };
      send({ type: "response.created", response: { status: "in_progress" } });
      if (fixture === "heartbeat") {
        const timer = setInterval(() => response.write(": heartbeat\n\n"), 100);
        response.once("close", () => clearInterval(timer));
        await sleep(1_600);
        if (!response.destroyed) response.destroy();
        return;
      }
      send(delta);
      if (fixture === "idle") {
        await sleep(2_500);
        if (!response.destroyed) response.destroy();
        return;
      }
      if (fixture === "overload") {
        send({ type: "response.failed", response: { error: { code: "server_overloaded" } } });
        return response.end();
      }
      if (provider === "first" && model === sol && failSol && fixture.startsWith("drop")) {
        await sleep(80);
        return response.end();
      }
      if (provider === "first" && model === sol && fixture === "recover" && !heldRecovery) {
        heldRecovery = true;
        reachedRecovery.resolve();
        await completeRecovery.promise;
      }
      send(done);
      response.end();
    } catch (error) {
      upstreamError ??= error;
      response.destroy();
    }
  });
  return server;
}

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return server.address().port;
}

async function reservePort() {
  const server = net.createServer();
  return { server, port: await listen(server) };
}

function readDiagnostics() {
  const log = path.join(state, "codex-plus.log");
  if (!fs.existsSync(log)) return [];
  return fs.readFileSync(log, "utf8").split(/\r?\n/).flatMap(line => {
    try { return [JSON.parse(line)]; } catch { return []; }
  });
}

async function until(check, ms) {
  const deadline = Date.now() + ms;
  while (!check()) {
    assert.ok(Date.now() < deadline, `fixture deadline reached in ${root}`);
    await sleep(75);
  }
}

const servers = ["first", "backup"].map(createUpstream);
const upstreamPorts = await Promise.all(servers.map(listen));
const reservations = await Promise.all(Array.from({ length: 3 }, reservePort));
const [port, managerGuard, launcherGuard] = reservations.map(item => item.port);
fs.writeFileSync(path.join(codexHome, "config.toml"), "check_for_update_on_startup = false\n");
fs.writeFileSync(path.join(state, "gateway.json"), JSON.stringify({
  version: 6, autoModels: true, restoreOnExit: false, responsesWebsocket: false,
  bindings: { codex: { enabled: true, model: sol, token: "local-test" } },
  providers: ["first", "backup"].map((provider, index) => {
    const url = `http://127.0.0.1:${upstreamPorts[index]}/v1`;
    return { enabled: true, relay: {
      id: provider, name: provider, upstreamBaseUrl: url,
      configContents: `model_provider = "${provider}"\n[model_providers.${provider}]\nname = "${provider}"\nwire_api = "responses"\nbase_url = "${url}"\n`,
      apiKey: "fixture-only", relayMode: "pureApi", protocol: "responses", modelList: `${sol},${astra}`,
    } };
  }),
}));
const settingsFile = path.join(state, "settings.json");
const settings = {
  enhancementsEnabled: false, watcherStartCodex: false, watcherStartCodexOnOpen: false, managerAgentFocus: "all",
  gatewayStreamHealthRoutingEnabled: true, gatewayStreamFirstOutputTimeoutSecs: 1, gatewayStreamBodyIdleTimeoutSecs: 2,
};
fs.writeFileSync(settingsFile, JSON.stringify(settings));
for (const { server } of reservations) server.close();
const gateway = spawn(executable, [], {
  cwd: path.dirname(executable), stdio: "ignore", windowsHide: true,
  env: { ...process.env, CODEX_PLUS_STATE_DIR: state, CODEX_HOME: codexHome, CODEX_SQLITE_HOME: codexHome,
    CODEX_PLUS_PROTOCOL_PROXY_PORT: String(port), CODEX_PLUS_MANAGER_GUARD_PORT: String(managerGuard),
    CODEX_PLUS_LAUNCHER_GUARD_PORT: String(launcherGuard), WEBVIEW2_USER_DATA_FOLDER: path.join(root, "webview") },
});

async function send(model, fixture, expectedEvent = "response.completed", expectedProvider = "first", extra = {}) {
  const response = await fetch(`http://127.0.0.1:${port}/gw/codex/v1/responses`, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: "Bearer local-test" },
    body: JSON.stringify({ model, stream: true, ...extra,
      input: [{ type: "message", role: "user", content: [{ type: "input_text", text: fixture }] }] }),
    signal: AbortSignal.timeout(5_000),
  });
  const body = await response.text();
  const expectedStatus = expectedEvent === "gateway_upstream_header_timeout" ? 400 : 200;
  assert.equal(response.status, expectedStatus, body);
  assert.ok(body.includes(expectedEvent), body);
  if (expectedEvent === "response.failed") assert.ok(body.includes("invalid_prompt"), body);
  const upstreamModel = model.split("/").at(-1);
  assert.equal(counts.get(`${expectedProvider}/${upstreamModel}/${fixture}`), 1);
  assert.equal(counts.get(`${expectedProvider === "first" ? "backup" : "first"}/${upstreamModel}/${fixture}`), undefined);
}

try {
  await until(() => readDiagnostics().some(event => event.pid === gateway.pid &&
    event.event === "manager.resident_helper.ready" && event.detail.port === port), 25_000);
  await send(sol, "drop-one", "response.failed");
  await send(sol, "drop-two", "response.failed");
  const openAt = Date.now();
  await send(sol, "cooldown", "response.completed", "backup");
  await send(astra, "independent");
  await send(`first/${sol}`, "explicit");
  await send(sol, "stateful", "response.completed", "first", { previous_response_id: "resp_fixture" });
  await send(`first/${astra}`, "heartbeat", "response.failed");
  await send(`first/${astra}`, "idle", "response.failed");
  await send(`first/${astra}`, "overload", "response.failed");
  failSol = false;
  await sleep(Math.max(0, 30_500 - (Date.now() - openAt)));
  const recovering = send(sol, "recover");
  void recovering.catch(() => {});
  await Promise.race([
    reachedRecovery.promise,
    recovering.then(() => assert.fail("recovery returned before the held probe")),
    sleep(3_000).then(() => assert.fail("recovery probe never reached the preferred provider")),
  ]);
  try {
    await send(sol, "concurrent", "response.completed", "backup");
  } finally {
    completeRecovery.resolve();
  }
  await recovering;
  await send(sol, "restored");
  fs.writeFileSync(settingsFile, JSON.stringify({
    ...settings, gatewayStreamHeaderBudgetSecs: 1, gatewayProviderStreamHeaderTimeoutSecs: { first: 5 },
  }));
  await send(astra, "route-budget-one", "gateway_upstream_header_timeout");
  await send(astra, "route-budget-two", "gateway_upstream_header_timeout");
  await send(astra, "after-route-budget");
  if (upstreamError) throw upstreamError;
  const events = readDiagnostics();
  const outcomes = events.filter(event => event.event === "gateway.http_stream_outcome").map(event => event.detail);
  const heartbeat = outcomes.find(outcome => outcome.outcome === "first_output_timeout");
  assert.ok(heartbeat?.firstBodyMs >= heartbeat?.headersMs);
  assert.equal(heartbeat.firstOutputMs, null);
  assert.ok(outcomes.some(outcome => outcome.outcome === "body_idle_timeout" && outcome.firstOutputMs !== null));
  assert.ok(outcomes.some(outcome => outcome.outcome === "upstream_overloaded"));
  assert.equal(outcomes.filter(outcome => outcome.outcome === "route_header_timeout").length, 2);
  assert.equal(outcomes.filter(outcome => outcome.halfOpen && outcome.outcome === "completed").length, 1);
  assert.ok(events.filter(event => event.event === "gateway.http_stream_candidate_skipped").length >= 2);
  console.log(JSON.stringify({ root, checks: counts.size, halfOpenCompleted: 1, outcomes: outcomes.map(outcome => outcome.outcome) }));
} finally {
  completeRecovery.resolve();
  if (gateway.exitCode === null) gateway.kill();
  for (const server of servers) {
    server.closeAllConnections();
    server.close();
  }
}
