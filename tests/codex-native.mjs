// Run: node tests/codex-native.mjs /absolute/path/to/codex.exe
// Local fixture only: no real credentials, user config, or upstream inference.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync, rmdirSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createInterface } from 'node:readline';

assert(process.argv[2], 'Supply installed Codex backend path');
const home = mkdtempSync(join(tmpdir(), 'auranion-native-'));
const requests = [];
const server = createServer(async (req, res) => {
  let body = '';
  for await (const chunk of req) body += chunk;
  if (req.url !== '/v1/responses') {
    res.writeHead(404).end();
    return;
  }
  requests.push({ body: JSON.parse(body), authorization: req.headers.authorization });
  const response = { id: `resp_${requests.length}`, object: 'response', status: 'completed',
    output: [], usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2 } };
  res.writeHead(200, { 'Content-Type': 'text/event-stream' });
  res.end(`event: response.completed\ndata: ${JSON.stringify({ type: 'response.completed', response })}\n\n`);
});
await new Promise(done => server.listen(0, '127.0.0.1', done));
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) =>
  /^(PATH|SystemRoot|WINDIR|TEMP|TMP|PATHEXT|COMSPEC|CARGO_HOME|RUSTUP_HOME|USERPROFILE|LOCALAPPDATA|APPDATA)$/i.test(key)));
let child;
let stderr = '';
const pending = new Map();
const notifications = [];
let nextId = 0;
function request(method, params) {
  const id = ++nextId;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`Timed out: ${method}\n${stderr}`)); }, 30_000);
    pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
    child.stdin.write(JSON.stringify({ id, method, params }) + '\n');
  });
}
try {
  const fixture = spawnSync('cargo', ['test', 'config::adapters::codex::tests::export_native_fixture', '--', '--ignored', '--exact', '--nocapture'], {
    env: { ...env, AURANION_TEST_HOME: home, AURANION_TEST_URL: `http://127.0.0.1:${server.address().port}/v1`, AURANION_TEST_NODE: process.execPath },
    encoding: 'utf8', timeout: 120_000,
  });
  assert.equal(fixture.status, 0, fixture.stdout + fixture.stderr);
  assert(readFileSync(join(home, 'config.toml'), 'utf8').includes('model_provider = "auranion"'));
  child = spawn(resolve(process.argv[2]), ['app-server', '--stdio', '--strict-config'], {
    cwd: home, env: { ...env, CODEX_HOME: home, HOME: home, USERPROFILE: home, APPDATA: home, LOCALAPPDATA: home },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  child.stderr.on('data', chunk => { stderr += chunk; });
  child.on('error', error => { for (const p of pending.values()) p.reject(error); pending.clear(); });
  child.on('exit', code => { for (const p of pending.values()) p.reject(new Error(`app-server exited ${code}\n${stderr}`)); pending.clear(); });
  createInterface({ input: child.stdout }).on('line', line => {
    const message = JSON.parse(line);
    const p = pending.get(message.id);
    if (p) {
      pending.delete(message.id);
      message.error ? p.reject(new Error(JSON.stringify(message.error))) : p.resolve(message.result);
    } else notifications.push(message);
  });
  await request('initialize', { clientInfo: { name: 'auranion_config_test', version: '1.0.0' }, capabilities: { experimentalApi: true } });
  child.stdin.write(JSON.stringify({ method: 'initialized', params: {} }) + '\n');
  const { config } = await request('config/read', { includeLayers: false, cwd: home });
  assert.equal(config.model_provider, 'auranion');
  const { data: models } = await request('model/list', { includeHidden: false, cursor: null, limit: 100 });
  assert.deepEqual(models.map(m => m.model), ['gpt-6-astra', 'gpt-5.6-sol', 'gpt-5.6-terra', 'gpt-5.6-luna']);
  const started = await request('thread/start', { cwd: home, ephemeral: true, approvalPolicy: 'never', sandbox: 'read-only' });
  assert.equal(started.modelProvider, 'auranion');
  for (const model of models) {
    const efforts = model.supportedReasoningEfforts.map(e => e.reasoningEffort);
    assert.deepEqual(efforts, model.model === 'gpt-5.6-luna'
      ? ['low', 'medium', 'high', 'xhigh', 'max'] : ['low', 'medium', 'high', 'xhigh', 'max', 'ultra']);
    assert.equal(model.defaultReasoningEffort, 'medium');
    for (const effort of efforts) {
      const result = await request('turn/start', { threadId: started.thread.id, model: model.model, effort,
        input: [{ type: 'text', text: 'Local fixture check. Do not call tools.', text_elements: [] }] });
      const deadline = Date.now() + 30_000;
      let complete;
      while (!(complete = notifications.find(n => n.method === 'turn/completed' && n.params.turn.id === result.turn.id))) {
        assert(Date.now() < deadline, `Turn timed out: ${model.model}/${effort}\n${stderr}`);
        await new Promise(done => setTimeout(done, 20));
      }
      assert.equal(complete.params.turn.status, 'completed', JSON.stringify(complete));
      const sent = requests.at(-1);
      assert.equal(sent.authorization, 'Bearer fixture-token');
      assert.equal(sent.body.model, model.model);
      // Native Codex treats Ultra as orchestration mode and sends max upstream.
      assert.equal(sent.body.reasoning.effort, effort === 'ultra' ? 'max' : effort);
    }
  }
  assert.equal(requests.length, 23);
  assert(!notifications.some(n => /deprecat/i.test(JSON.stringify(n))), 'Deprecated configuration warning');
  console.log('PASS: native provider, four-model order, command auth, and all 23 model/effort requests');
} finally {
  if (child && child.exitCode === null) {
    const exited = new Promise(done => child.once('exit', done));
    if (process.platform === 'win32') {
      // Kill only this fixture's tree; native helpers can keep SQLite/temp files open.
      spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
    } else child.stdin.end();
    const timeout = setTimeout(() => child.kill(), 3_000);
    await exited;
    clearTimeout(timeout);
  }
  await new Promise(done => server.close(done));
  try {
    for (const name of readdirSync(home)) rmSync(join(home, name), { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    rmdirSync(home);
  }
  catch (error) { console.error(`Fixture cleanup failed: ${error.message}`); process.exitCode = 1; }
}
