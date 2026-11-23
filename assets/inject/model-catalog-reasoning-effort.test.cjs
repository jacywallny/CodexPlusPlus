const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const path = require("node:path");
const { test } = require("node:test");
const vm = require("node:vm");

function readFunctions(file, startMarker, endMarker) {
  const source = readFileSync(path.join(__dirname, "renderer-inject", file), "utf8");
  const start = source.indexOf(startMarker);
  assert.notEqual(start, -1);
  const end = source.indexOf(endMarker, start);
  assert.ok(end > start);
  return source.slice(start, end);
}

const modelNameFunctions = readFunctions("20-menu.js", "  function codexServiceTierModelFromValue", "  function codexServiceTierCurrentModelName");
const descriptorFunctions = readFunctions("70-model-catalog.js", "  function codexPlusModelMetadata", "  function modelArrayLooksPatchable");

function effortNames(descriptor) {
  return Array.from(descriptor.supportedReasoningEfforts, (entry) => entry.reasoningEffort);
}

function effortOptions(...names) {
  return names.map((reasoningEffort) => ({ reasoningEffort, description: `${reasoningEffort} effort` }));
}

function modelContext(modelMetadata) {
  const ctx = vm.createContext({ codexModelCatalog: { modelMetadata } });
  vm.runInContext(`${modelNameFunctions}\n${descriptorFunctions}`, ctx);
  return ctx;
}

test("GPT-6.2 裸模型使用目录档位并移除原生描述符上的额外档位", () => {
  const model = "gpt-6.2-astra";
  const supportedReasoningEfforts = effortOptions("low", "medium", "high", "xhigh");
  const ctx = modelContext({ [model]: { supportedReasoningEfforts } });

  const created = ctx.codexPlusModelDescriptor(model);
  assert.deepEqual(effortNames(created), ["low", "medium", "high", "xhigh"]);
  assert.notEqual(created.supportedReasoningEfforts[0], supportedReasoningEfforts[0]);

  const existing = { supportedReasoningEfforts: effortOptions("low", "medium", "high", "xhigh", "max", "ultra") };
  assert.equal(ctx.applyCodexPlusModelMetadata(existing, model), true);
  assert.deepEqual(effortNames(existing), ["low", "medium", "high", "xhigh"]);
  assert.equal(ctx.applyCodexPlusModelMetadata(existing, model), false);
});

test("GPT-6.2 供应商限定模型保留目录声明的 Ultra 档位", () => {
  const model = "nanlab/gpt-6.2-sol";
  const ctx = modelContext({ [model]: { supportedReasoningEfforts: effortOptions("low", "medium", "high", "xhigh", "ultra") } });

  assert.deepEqual(effortNames(ctx.codexPlusModelDescriptor(model)), ["low", "medium", "high", "xhigh", "ultra"]);
});

test("GPT-6.2 空档位声明覆盖已有档位，缺少声明时保留原生档位", () => {
  const emptyModel = "nanlab/gpt-6.2-astra";
  const unspecifiedModel = "gpt-6.2-sol";
  const ctx = modelContext({ [emptyModel]: { supportedReasoningEfforts: [] }, [unspecifiedModel]: {} });

  assert.deepEqual(effortNames(ctx.codexPlusModelDescriptor(emptyModel)), []);
  const existing = { supportedReasoningEfforts: effortOptions("medium") };
  assert.equal(ctx.applyCodexPlusModelMetadata(existing, unspecifiedModel), false);
  assert.deepEqual(effortNames(existing), ["medium"]);
  assert.equal(ctx.applyCodexPlusModelMetadata(existing, emptyModel), true);
  assert.deepEqual(effortNames(existing), []);
});

test("旧模型沿用 Max 和 Ultra 档位补全", () => {
  const model = "gpt-6.1-sol";
  const ctx = modelContext({ [model]: { supportedReasoningEfforts: effortOptions("low", "medium", "high", "xhigh") } });

  assert.deepEqual(effortNames(ctx.codexPlusModelDescriptor(model)), ["low", "medium", "high", "xhigh", "max", "ultra"]);
});
