import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));

const releaseYml = readFileSync(join(root, '.github', 'workflows', 'release.yml'), 'utf8');
const installSh = readFileSync(join(root, 'install.sh'), 'utf8');
const installPs1 = readFileSync(join(root, 'install.ps1'), 'utf8');

// 1. Validate release.yml matrix & triggers
assert(/['"]?on['"]?:\s*\n\s*push:\s*\n\s*branches:\s*\n\s*-\s*main\s*\n\s*pull_request:/m.test(releaseYml),
  'release.yml must trigger on push and pull_request to main');
assert(releaseYml.includes('fail-fast: false'), 'release.yml matrix should specify fail-fast: false');

const expectedTargets = [
  { os: 'windows-latest', target: 'x86_64-pc-windows-msvc', asset: 'auranion-windows-amd64.exe' },
  { os: 'windows-11-arm', target: 'aarch64-pc-windows-msvc', asset: 'auranion-windows-arm64.exe' },
  { os: 'macos-latest', target: 'aarch64-apple-darwin', asset: 'auranion-macos-arm64' },
  { os: 'macos-15-intel', target: 'x86_64-apple-darwin', asset: 'auranion-macos-amd64' },
  { os: 'ubuntu-22.04', target: 'x86_64-unknown-linux-gnu', asset: 'auranion-linux-amd64' },
  { os: 'ubuntu-22.04-arm', target: 'aarch64-unknown-linux-gnu', asset: 'auranion-linux-arm64' },
];

for (const t of expectedTargets) {
  assert(releaseYml.includes(t.os), `Missing OS runner ${t.os} in release.yml`);
  assert(releaseYml.includes(t.target), `Missing target ${t.target} in release.yml`);
  assert(releaseYml.includes(t.asset), `Missing asset ${t.asset} in release.yml`);
}

assert(releaseYml.includes('cargo test --locked --target ${{ matrix.target }}'),
  'release.yml must run cargo test --locked before release build');
assert(releaseYml.includes('cargo build --release --locked --target ${{ matrix.target }}'),
  'release.yml must run cargo build --release --locked');
assert(releaseYml.includes('if: github.event_name == \'push\' && github.ref == \'refs/heads/main\''),
  'release.yml create-release must only publish on main push');

// 2. Validate install.sh logic & safety
assert(installSh.includes('sysctl.proc_translated'), 'install.sh must check Rosetta via sysctl.proc_translated');
assert(installSh.includes('mktemp'), 'install.sh must stage download via mktemp');
assert(installSh.includes('trap '), 'install.sh must set cleanup trap');
assert(installSh.includes('--version'), 'install.sh must verify downloaded binary via --version');
assert(installSh.includes('auranion-macos-arm64'), 'install.sh must include auranion-macos-arm64');
assert(installSh.includes('auranion-macos-amd64'), 'install.sh must include auranion-macos-amd64');
assert(installSh.includes('auranion-linux-arm64'), 'install.sh must include auranion-linux-arm64');
assert(installSh.includes('auranion-linux-amd64'), 'install.sh must include auranion-linux-amd64');
assert(!installSh.includes('schedule'), 'install.sh must not register scheduled tasks');

// Syntax check with sh -n
const shCheck = spawnSync('sh', ['-n', join(root, 'install.sh')]);
assert.equal(shCheck.status, 0, `install.sh syntax check failed: ${shCheck.stderr}`);

// 3. Validate install.ps1 logic & safety
assert(installPs1.includes('IsWow64Process2'), 'install.ps1 must query IsWow64Process2');
assert(installPs1.includes('0xAA64'), 'install.ps1 must match IMAGE_FILE_MACHINE_ARM64 (0xAA64)');
assert(installPs1.includes('0x8664'), 'install.ps1 must match IMAGE_FILE_MACHINE_AMD64 (0x8664)');
assert(installPs1.includes('auranion-windows-arm64.exe'), 'install.ps1 must support auranion-windows-arm64.exe');
assert(installPs1.includes('auranion-windows-amd64.exe'), 'install.ps1 must support auranion-windows-amd64.exe');
assert(installPs1.includes('GetRandomFileName'), 'install.ps1 must stage download via GetRandomFileName');
assert(installPs1.includes('$LASTEXITCODE'), 'install.ps1 must check $LASTEXITCODE for native execution failure');
assert(installPs1.includes('Move-Item'), 'install.ps1 must move staged binary only on success');
assert(installPs1.includes('finally'), 'install.ps1 must clean up staged binary in finally');
assert(installPs1.includes('SendMessageTimeout'), 'install.ps1 must preserve WM_SETTINGCHANGE broadcast');
assert(!installPs1.includes('schedule'), 'install.ps1 must not register scheduled tasks');

// 4. Mock simulation: test install.sh resolution for various OS/ARCH matrix
const testCases = [
  { os: 'Darwin', arch: 'arm64', rosetta: '0', expected: 'auranion-macos-arm64' },
  { os: 'Darwin', arch: 'x86_64', rosetta: '1', expected: 'auranion-macos-arm64' },
  { os: 'Darwin', arch: 'x86_64', rosetta: '0', expected: 'auranion-macos-amd64' },
  { os: 'Linux', arch: 'x86_64', rosetta: '0', expected: 'auranion-linux-amd64' },
  { os: 'Linux', arch: 'aarch64', rosetta: '0', expected: 'auranion-linux-arm64' },
];

for (const tc of testCases) {
  const script = `
OS="${tc.os}"
ARCH="${tc.arch}"
sysctl() { echo "${tc.rosetta}"; }
` + installSh.slice(installSh.indexOf('case "$OS" in'), installSh.indexOf('mkdir -p "$INSTALL_DIR"')) + `
echo "$ASSET"
`;
  const res = spawnSync('sh', ['-c', script]);
  assert.equal(res.status, 0, `Mock run failed for ${JSON.stringify(tc)}: ${res.stderr}`);
  assert.equal(res.stdout.toString().trim(), tc.expected, `Mismatch for ${JSON.stringify(tc)}`);
}

// Ensure unknown arch is rejected in install.sh
const rejectCases = [
  { os: 'Darwin', arch: 'mips' },
  { os: 'Linux', arch: 'riscv64' },
  { os: 'FreeBSD', arch: 'x86_64' },
];
for (const tc of rejectCases) {
  const script = `
OS="${tc.os}"
ARCH="${tc.arch}"
sysctl() { echo "0"; }
` + installSh.slice(installSh.indexOf('case "$OS" in'), installSh.indexOf('mkdir -p "$INSTALL_DIR"')) + `
echo "$ASSET"
`;
  const res = spawnSync('sh', ['-c', script]);
  assert.notEqual(res.status, 0, `Expected rejection for ${JSON.stringify(tc)}`);
}

console.log('All installer checks passed.');
