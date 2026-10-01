// Research fixture only, not a production launcher or automatic recovery route.
// Use Pi's PUBLIC native TUI and Terminal interface, not private method patches.
import { appendFileSync, fstatSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

const packageDir = process.env.PI_PACKAGE_DIR;
const requirePi = createRequire(join(packageDir, 'package.json'));
const pi = await import(pathToFileURL(join(packageDir, 'dist/index.js')));
const { ProcessTerminal } = await import(pathToFileURL(requirePi.resolve('@earendil-works/pi-tui')));
// This inherited kernel lock belongs to the backend, not its terminal client.
// It only fences launchers participating in this managed route, NOT bare Pi.
fstatSync(Number(process.env.PI_OWNED_LOCK_FD));
const mark = (event, extra = {}) => appendFileSync(process.env.PI_OWNED_EVENTS,
  JSON.stringify({ event, pid: process.pid, ...extra }) + '\n');

const factory = async ({ cwd, sessionManager, sessionStartEvent }) => {
  const services = await pi.createAgentSessionServices({
    cwd, agentDir: process.env.PI_CODING_AGENT_DIR,
    resourceLoaderOptions: { noExtensions: true, noSkills: true, noPromptTemplates: true,
      noContextFiles: true,
      additionalExtensionPaths: [process.env.PI_OWNED_PROVIDER,
        ...(process.env.PI_OWNED_DIALOGS ? [process.env.PI_OWNED_DIALOGS] : [])] },
  });
  const result = await pi.createAgentSessionFromServices({ services, sessionManager,
    sessionStartEvent, model: services.modelRuntime.getModel('intent-fixture', 'fixture'),
    thinkingLevel: 'off', noTools: 'all' });
  result.session.agent.subscribe((event, signal) => {
    if (event.type === 'agent_start') {
      mark('native_agent_start', { sessionId: sessionManager.getSessionId(),
        sessionFile: sessionManager.getSessionFile() });
      signal.addEventListener('abort', () => mark('native_abort_signal'), { once: true });
    }
  });
  return { ...result, services, diagnostics: services.diagnostics };
};
const manager = pi.SessionManager.open(process.env.PI_OWNED_SESSION);
const runtime = await pi.createAgentSessionRuntime(factory, {
  cwd: manager.getCwd(), agentDir: process.env.PI_CODING_AGENT_DIR, sessionManager: manager,
});
const nativeTerminal = new ProcessTerminal();
const terminal = new Proxy(nativeTerminal, {
  get(target, key) {
    if (key === 'start') return (onInput, onResize) => target.start(data => {
      const session = runtime.session;
      // Conservatively hold on ALL input during work, not inferred key meanings.
      // Write through Reignite's existing fsynced policy store BEFORE forwarding.
      // Failure throws BEFORE Pi sees input; no automatic clearing or rearming.
      if (!session.isIdle) {
        execFileSync(process.env.PI_OWNED_REIGNITE,
          ['--state-dir', process.env.PI_OWNED_STATE, 'disable', '--session', session.sessionFile],
          { stdio: ['ignore', 'pipe', 'pipe'], timeout: 10000, maxBuffer: 1024 * 1024 });
        mark('policy_saved_before_input');
      }
      onInput(data);
    }, onResize);
    const value = Reflect.get(target, key, target);
    return typeof value === 'function' ? value.bind(target) : value;
  },
});
mark('host_ready', { sessionId: manager.getSessionId(), sessionFile: manager.getSessionFile() });
// This is Pi's own TUI, not a renderer or RPC-to-TUI attachment built by Reignite.
const mode = new pi.InteractiveMode(runtime, { terminal, tuiMode: 'regular' });
await mode.run();
