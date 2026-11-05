const INSTALLERS = {
  windows: 'Windows_x64-setup.exe',
  'windows-msi': 'Windows_x64.msi',
  'macos-arm': 'macOS_arm64.dmg',
  'macos-intel': 'macOS_x64.dmg',
  'linux-deb': 'Linux_x64.deb',
  'linux-rpm': 'Linux_x64.rpm',
  'linux-arm64-deb': 'Linux_arm64.deb',
  'linux-arm64-rpm': 'Linux_arm64.rpm',
};

const MIRRORS = [
  {
    id: 'cnb',
    manifest: 'https://cnb.cool/echobird/echobird-releases/-/git/raw/main/manifest.json',
    file: (name) =>
      `https://cnb.cool/echobird/echobird-releases/-/git/raw/main/${encodeURIComponent(name)}`,
  },
  {
    id: 'gitcode',
    manifest:
      'https://api.gitcode.com/api/v5/repos/edison7009/echobird-releases/raw/manifest.json?ref=main',
    file: (name) =>
      `https://api.gitcode.com/api/v5/repos/edison7009/echobird-releases/raw/${encodeURIComponent(name)}?ref=main`,
  },
];

const mirrorCache = new Map();

async function getMirrorManifest(mirror) {
  const cached = mirrorCache.get(mirror.id);
  if (cached && Date.now() - cached.at < (cached.manifest ? 300000 : 120000))
    return cached.manifest;
  let manifest = null;
  try {
    const response = await fetch(mirror.manifest, {
      headers: { 'User-Agent': 'EchoBird-Download' },
      signal: AbortSignal.timeout(4000),
    });
    if (response.ok) {
      const value = await response.json();
      if (
        value &&
        typeof value.version === 'string' &&
        value.files &&
        typeof value.files === 'object'
      )
        manifest = value;
    }
  } catch {
    /* a failed mirror is a normal fallback case */
  }
  mirrorCache.set(mirror.id, { at: Date.now(), manifest });
  return manifest;
}

async function findMirror(asset, only) {
  for (const mirror of MIRRORS) {
    if (only && mirror.id !== only) continue;
    const manifest = await getMirrorManifest(mirror);
    if (manifest?.version === asset.version && manifest.files[asset.name]) {
      return { mirror, size: manifest.files[asset.name].size };
    }
  }
  return null;
}

function newerVersion(left, right) {
  const a = String(left).replace(/^v/, '').split('.').map(Number);
  const b = String(right).replace(/^v/, '').split('.').map(Number);
  for (let i = 0; i < 3; i += 1) {
    if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) > (b[i] ?? 0) ? left : right;
  }
  return left;
}

async function latestVersion(env, includeMirrors) {
  const officialPromise = env.ASSETS.fetch(
    new Request('https://echobird.ai/api/version/index.json')
  ).then(async (response) => (response.ok ? (await response.json()).version : null));
  if (!includeMirrors) return await officialPromise;
  const [official, mirrors] = await Promise.all([
    officialPromise,
    Promise.all(MIRRORS.map((mirror) => getMirrorManifest(mirror))),
  ]);
  return mirrors.reduce(
    (best, manifest) =>
      manifest?.version ? newerVersion(manifest.version, best || manifest.version) : best,
    official || null
  );
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (!url.pathname.startsWith('/download/')) return env.ASSETS.fetch(request);

    const platform = url.pathname.slice('/download/'.length);
    const installer = Object.hasOwn(INSTALLERS, platform) ? INSTALLERS[platform] : null;
    if (!installer) return new Response('Unknown download platform.', { status: 404 });
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      return new Response('Method not allowed.', { status: 405, headers: { Allow: 'GET, HEAD' } });
    }

    try {
      const source = url.searchParams.get('source');
      const wantsMirrors = source === 'cnb' || source === 'gitcode' || request.cf?.country === 'CN';
      const version = await latestVersion(env, wantsMirrors);
      if (!version) throw new Error('Version missing');

      const filename = `EchoBird_${version}_${installer}`;
      if (
        source !== 'github' &&
        (source === 'cnb' || source === 'gitcode' || request.cf?.country === 'CN')
      ) {
        const mirror = await findMirror(
          { version, name: filename },
          source === 'cnb' || source === 'gitcode' ? source : ''
        );
        if (mirror) {
          return new Response(null, {
            status: 302,
            headers: {
              Location: mirror.mirror.file(filename),
              'Cache-Control': 'no-store',
              'X-EchoBird-Version': version,
              'X-EchoBird-Source': mirror.mirror.id,
              'X-EchoBird-Size': String(mirror.size),
            },
          });
        }
      }
      const response = await fetch(
        `https://github.com/edison7009/EchoBird/releases/download/v${version}/${filename}`,
        { method: request.method, headers: { 'User-Agent': 'EchoBird-Download' } }
      );
      if (!response.ok) {
        return new Response('Installer unavailable. Please try again later.', {
          status: response.status,
        });
      }

      const headers = new Headers({
        'Content-Type': 'application/octet-stream',
        'Content-Disposition': `attachment; filename="${filename}"`,
        'Cache-Control': 'no-store',
      });
      const length = response.headers.get('Content-Length');
      if (length) headers.set('Content-Length', length);
      return new Response(request.method === 'HEAD' ? null : response.body, { headers });
    } catch {
      return new Response('Download temporarily unavailable. Please try again later.', {
        status: 503,
      });
    }
  },
};
