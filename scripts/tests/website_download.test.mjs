import assert from 'node:assert/strict';
import { afterEach, mock, test } from 'node:test';
import worker from '../../docs/_worker.js';

afterEach(() => mock.restoreAll());

function createEnv(response = () => Response.json({ version: '6.0.2' })) {
  return { ASSETS: { fetch: mock.fn(async () => response()) } };
}

for (const [platform, filename] of [
  ['windows', 'EchoBird_6.0.2_Windows_x64-setup.exe'],
  ['macos-arm', 'EchoBird_6.0.2_macOS_arm64.dmg'],
  ['linux-deb', 'EchoBird_6.0.2_Linux_x64.deb'],
]) {
  test(`${platform} streams the published installer as an attachment`, async () => {
    const upstream = mock.method(globalThis, 'fetch', async (url, options) => {
      assert.equal(
        url,
        `https://github.com/edison7009/EchoBird/releases/download/v6.0.2/${filename}`
      );
      assert.equal(options.method, 'GET');
      return new Response('installer bytes', { headers: { 'Content-Length': '15' } });
    });
    const env = createEnv();
    const response = await worker.fetch(
      new Request(`https://echobird.ai/download/${platform}`),
      env
    );
    assert.equal(response.status, 200);
    assert.equal(response.headers.get('Content-Disposition'), `attachment; filename="${filename}"`);
    assert.equal(response.headers.get('Content-Type'), 'application/octet-stream');
    assert.equal(response.headers.get('Content-Length'), '15');
    assert.equal(response.headers.get('Cache-Control'), 'no-store');
    assert.equal(response.headers.get('Location'), null);
    assert.equal(await response.text(), 'installer bytes');
    assert.equal(upstream.mock.callCount(), 1);
    const manifestRequest = env.ASSETS.fetch.mock.calls[0].arguments[0];
    assert.equal(manifestRequest.url, 'https://echobird.ai/api/version/index.json');
    assert.equal(manifestRequest.method, 'GET');
  });
}

test('the download follows the published version manifest', async () => {
  mock.method(globalThis, 'fetch', async (url) => {
    assert.equal(
      url,
      'https://github.com/edison7009/EchoBird/releases/download/v6.1.3/EchoBird_6.1.3_Windows_x64-setup.exe'
    );
    return new Response('new installer');
  });
  const response = await worker.fetch(
    new Request('https://echobird.ai/download/windows'),
    createEnv(() => Response.json({ version: '6.1.3' }))
  );
  assert.equal(response.status, 200);
  assert.equal(
    response.headers.get('Content-Disposition'),
    'attachment; filename="EchoBird_6.1.3_Windows_x64-setup.exe"'
  );
});

test('mainland requests redirect to the current CNB mirror asset', async () => {
  const calls = [];
  mock.method(globalThis, 'fetch', async (url) => {
    calls.push(String(url));
    if (String(url).includes('cnb.cool')) {
      return Response.json({
        version: '6.2.0',
        files: { 'EchoBird_6.2.0_Windows_x64-setup.exe': { size: 12, sha256: 'x' } },
      });
    }
    throw new Error(`unexpected fetch ${url}`);
  });
  const request = new Request('https://echobird.ai/download/windows');
  Object.defineProperty(request, 'cf', { value: { country: 'CN' } });
  const response = await worker.fetch(
    request,
    createEnv(() => Response.json({ version: '6.2.0' }))
  );
  assert.equal(response.status, 302);
  assert.equal(response.headers.get('X-EchoBird-Source'), 'cnb');
  assert.equal(
    response.headers.get('Location'),
    'https://cnb.cool/echobird/echobird-releases/-/git/raw/main/EchoBird_6.2.0_Windows_x64-setup.exe'
  );
  assert.equal(calls.length, 2);
});

test('mainland requests use a mirror version newer than the website manifest', async () => {
  mock.method(globalThis, 'fetch', async (url) => {
    if (String(url).includes('cnb.cool')) {
      return Response.json({
        version: '6.2.0',
        files: { 'EchoBird_6.2.0_Windows_x64-setup.exe': { size: 12, sha256: 'x' } },
      });
    }
    if (String(url).includes('gitcode.com')) return new Response('missing', { status: 404 });
    throw new Error(`unexpected fetch ${url}`);
  });
  const request = new Request('https://echobird.ai/download/windows');
  Object.defineProperty(request, 'cf', { value: { country: 'CN' } });
  const response = await worker.fetch(
    request,
    createEnv(() => Response.json({ version: '6.1.9' }))
  );
  assert.equal(response.status, 302);
  assert.equal(response.headers.get('X-EchoBird-Version'), '6.2.0');
  assert.match(response.headers.get('Location'), /EchoBird_6\.2\.0_Windows_x64-setup\.exe$/);
});

test('source=github bypasses domestic mirrors', async () => {
  mock.method(globalThis, 'fetch', async (url) => {
    assert.equal(
      String(url),
      'https://github.com/edison7009/EchoBird/releases/download/v6.2.0/EchoBird_6.2.0_Windows_x64-setup.exe'
    );
    return new Response('installer');
  });
  const request = new Request('https://echobird.ai/download/windows?source=github');
  const response = await worker.fetch(
    request,
    createEnv(() => Response.json({ version: '6.2.0' }))
  );
  assert.equal(response.status, 200);
  assert.equal(await response.text(), 'installer');
});

test('HEAD checks the installer without transferring its body', async () => {
  mock.method(globalThis, 'fetch', async (_url, options) => {
    assert.equal(options.method, 'HEAD');
    return new Response(null, { headers: { 'Content-Length': '20791723' } });
  });
  const response = await worker.fetch(
    new Request('https://echobird.ai/download/windows', { method: 'HEAD' }),
    createEnv()
  );
  assert.equal(response.status, 200);
  assert.equal(response.headers.get('Content-Length'), '20791723');
  assert.equal(await response.text(), '');
});

test('normal website requests pass through with their headers intact', async () => {
  const request = new Request('https://echobird.ai/pulse/latest-7d.json');
  const asset = new Response('feed', { headers: { 'Access-Control-Allow-Origin': '*' } });
  const env = createEnv(() => asset);
  assert.equal(await worker.fetch(request, env), asset);
  assert.equal(env.ASSETS.fetch.mock.calls[0].arguments[0], request);
});

test('unknown platforms and unsupported methods do not fetch a release', async () => {
  const upstream = mock.method(globalThis, 'fetch', async () =>
    assert.fail('Unexpected release request')
  );
  const env = createEnv();
  for (const platform of ['unknown', 'toString', '__proto__']) {
    const response = await worker.fetch(
      new Request(`https://echobird.ai/download/${platform}`),
      env
    );
    assert.equal(response.status, 404);
    assert.equal(response.headers.get('Content-Disposition'), null);
  }
  const response = await worker.fetch(
    new Request('https://echobird.ai/download/windows', { method: 'POST' }),
    env
  );
  assert.equal(response.status, 405);
  assert.equal(response.headers.get('Allow'), 'GET, HEAD');
  assert.equal(upstream.mock.callCount(), 0);
  assert.equal(env.ASSETS.fetch.mock.callCount(), 0);
});

for (const [description, manifest] of [
  ['missing manifest', () => new Response('not found', { status: 404 })],
  ['invalid JSON', () => new Response('not JSON')],
  ['missing version', () => Response.json({})],
]) {
  test(`${description} returns an error without downloading an installer`, async () => {
    const upstream = mock.method(globalThis, 'fetch', async () =>
      assert.fail('Unexpected release request')
    );
    const response = await worker.fetch(
      new Request('https://echobird.ai/download/windows'),
      createEnv(manifest)
    );
    assert.equal(response.status, 503);
    assert.equal(response.headers.get('Content-Disposition'), null);
    assert.equal(upstream.mock.callCount(), 0);
  });
}

test('an unavailable installer does not save the upstream error as a package', async () => {
  mock.method(
    globalThis,
    'fetch',
    async () => new Response('<html>not found</html>', { status: 404 })
  );
  const response = await worker.fetch(
    new Request('https://echobird.ai/download/windows'),
    createEnv()
  );
  assert.equal(response.status, 404);
  assert.equal(response.headers.get('Content-Disposition'), null);
  assert.doesNotMatch(await response.text(), /<html>/);
});

test('network failure returns an error without attachment headers', async () => {
  mock.method(globalThis, 'fetch', async () => {
    throw new Error('offline');
  });
  const response = await worker.fetch(
    new Request('https://echobird.ai/download/windows'),
    createEnv()
  );
  assert.equal(response.status, 503);
  assert.equal(response.headers.get('Content-Disposition'), null);
});

for (const source of ['cnb', 'gitcode']) {
  for (const [platform, installer] of [
    ['windows', 'Windows_x64-setup.exe'],
    ['windows-msi', 'Windows_x64.msi'],
    ['macos-arm', 'macOS_arm64.dmg'],
    ['macos-intel', 'macOS_x64.dmg'],
    ['linux-deb', 'Linux_x64.deb'],
    ['linux-rpm', 'Linux_x64.rpm'],
    ['linux-arm64-deb', 'Linux_arm64.deb'],
    ['linux-arm64-rpm', 'Linux_arm64.rpm'],
  ]) {
    test(`${source} serves the matching ${platform} installer`, async () => {
      const { default: isolatedWorker } = await import(
        `../../docs/_worker.js?mirror=${source}&platform=${platform}`
      );
      const filename = `EchoBird_6.2.1_${installer}`;
      mock.method(globalThis, 'fetch', async () =>
        Response.json({
          version: '6.2.1',
          files: { [filename]: { size: 123, sha256: 'fixture' } },
        })
      );
      const response = await isolatedWorker.fetch(
        new Request(`https://echobird.ai/download/${platform}?source=${source}`),
        createEnv(() => Response.json({ version: '6.2.1' }))
      );
      assert.equal(response.status, 302);
      assert.equal(response.headers.get('X-EchoBird-Source'), source);
      const location = new URL(response.headers.get('Location'));
      assert.equal(location.hostname, source === 'cnb' ? 'cnb.cool' : 'api.gitcode.com');
      assert.equal(decodeURIComponent(location.pathname.split('/').at(-1)), filename);
      assert.equal(response.headers.get('X-EchoBird-Size'), '123');
    });
  }
}
