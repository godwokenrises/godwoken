// Run from any directory; both portal directories must already be exported by Rust.
import { spawn } from 'node:child_process';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
if (!process.argv[2] || !process.argv[3]) throw Error('Usage: node smoke-protocols.mjs V0_PORTAL_DIRECTORY V1_PORTAL_DIRECTORY');
const v0Portal = path.resolve(process.argv[2]);
const v1Portal = path.resolve(process.argv[3]);
const probe = net.createServer();
await new Promise(resolve => probe.listen(0, '127.0.0.1', resolve));
const port = probe.address().port;
await new Promise(resolve => probe.close(resolve));
const origin = `http://127.0.0.1:${port}`;
const child = spawn(process.env.GW_WITHDRAW_CLI ?? path.join(root, 'target/debug/gw-withdraw'), [
  'claim-serve', '--listen', `127.0.0.1:${port}`, '--public-origin', origin,
  '--tables', v1Portal, '--plan', path.join(v1Portal, 'plan.json'),
  '--v0-tables', v0Portal, '--v0-plan', path.join(v0Portal, 'plan.json'),
  '--web-dir', 'claim-web/dist', '--dependencies', 'claim-web/mainnet-dependencies.json',
], { cwd: root, stdio: ['ignore', 'ignore', 'inherit'] });
try {
  let ready = false;
  for (let i = 0; i < 120; ++i) {
    if (child.exitCode !== null) throw Error(`Server exited ${child.exitCode}`);
    try { if ((await fetch(origin + '/api/v1/config')).ok) { ready = true; break; } } catch {}
    await new Promise(resolve => setTimeout(resolve, 500));
  }
  if (!ready) throw Error('Server startup timeout');
  const v1 = await (await fetch(origin + '/api/v1/config')).json();
  const v0 = await (await fetch(origin + '/api/v0/config')).json();
  if (v1.protocol !== 'v1' || v0.protocol !== 'v0' || v1.plan_manifest === v0.plan_manifest) throw Error('Wrong protocol/manifest');
  if (v1.assets.find(a => a.asset_id === 1).decimals !== 18 || v0.assets.find(a => a.asset_id === 1).decimals !== 8) throw Error('Mixed CKB units');
  if ((await fetch(origin + '/api/config')).ok) throw Error('Unscoped route accepted');
  const response = await fetch(origin + '/api/v0/prepare', {
    method: 'POST', headers: { Origin: origin, 'Content-Type': 'application/json' },
    body: JSON.stringify({ protocol: 'v1', plan_manifest: v0.plan_manifest,
      request: { address: '0x' + '12'.repeat(20), destination: 'ckb1invalid', inputs: [] } }),
  });
  if (response.ok || !(await response.json()).error.includes('protocol')) throw Error('Cross-protocol request not rejected at scope validation');
  if (v1.broadcast_enabled || v0.broadcast_enabled) throw Error('Broadcast unexpectedly enabled');
  console.log(JSON.stringify({ real_public_datasets_loaded: true, v1_assets: v1.assets.length,
    v0_assets: v0.assets.length, v1_ckb_decimals: 18, v0_ckb_decimals: 8,
    explicit_routes: true, cross_protocol_rejected: true, broadcast_enabled: false }, null, 2));
} finally {
  child.kill();
}
