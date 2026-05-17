// echobird-pulse — Cloudflare Worker that proxies multiple GitHub data sources
// behind a single edge endpoint at echobird.ai. Pure pass-through with edge cache.
//
// Routes:
//   /pulse/<file>     → edison7009/EchoBird docs/pulse (refreshed every 6h by
//                       .github/workflows/refresh-pulse-data.yml — which mirrors
//                       the SuYxh/ai-news-aggregator ZH feed, builds the EN feed
//                       via scripts/build_en_pulse.py, and runs scripts/filter_pulse.py
//                       to strip x.com / twitter.com items).
//                       e.g. /pulse/latest-24h.json, /pulse/latest-7d.json,
//                            /pulse/latest-7d-en.json
//   /courses/<file>   → dair-ai/ML-YouTube-Courses (~80 curated AI courses)
//                       e.g. /courses/README.md
//
// To add a new data source, append an entry to ROUTES.

const ROUTES = [
    {
        prefix: '/pulse/',
        upstream: 'https://raw.githubusercontent.com/edison7009/EchoBird/main/docs/pulse',
        defaultFile: 'latest-24h.json',
        defaultContentType: 'application/json; charset=utf-8',
        ttl: file => {
            if (file === 'latest-24h.json') return 1800;       // 30 min
            if (file === 'latest-7d.json')  return 3600;       // 1 hour
            if (file === 'archive.json')    return 6 * 3600;   // 6 hours
            return 1800;
        },
    },
    {
        prefix: '/courses/',
        upstream: 'https://raw.githubusercontent.com/dair-ai/ML-YouTube-Courses/main',
        defaultFile: 'README.md',
        defaultContentType: 'text/markdown; charset=utf-8',
        ttl: () => 6 * 3600, // 6 hours — courses change slowly
    },
];

function corsHeaders() {
    return {
        'Access-Control-Allow-Origin':  '*',
        'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
        'Access-Control-Allow-Headers': 'Content-Type',
        'Access-Control-Max-Age':       '86400',
    };
}

function pickContentType(file, fallback) {
    const ext = (file.split('.').pop() || '').toLowerCase();
    if (ext === 'json') return 'application/json; charset=utf-8';
    if (ext === 'md')   return 'text/markdown; charset=utf-8';
    if (ext === 'xml')  return 'application/xml; charset=utf-8';
    return fallback;
}

export default {
    async fetch(request) {
        if (request.method === 'OPTIONS') {
            return new Response(null, { status: 204, headers: corsHeaders() });
        }
        if (request.method !== 'GET' && request.method !== 'HEAD') {
            return new Response('method not allowed', { status: 405, headers: corsHeaders() });
        }

        const url = new URL(request.url);
        const route = ROUTES.find(r => url.pathname.startsWith(r.prefix));
        if (!route) {
            return new Response('not found', { status: 404, headers: corsHeaders() });
        }

        let file = url.pathname.slice(route.prefix.length);
        if (file.includes('..') || file.startsWith('/')) {
            return new Response('bad path', { status: 400, headers: corsHeaders() });
        }
        if (file === '') file = route.defaultFile;

        const upstreamUrl = `${route.upstream}/${file}`;
        const ttl = route.ttl(file);

        const upstream = await fetch(upstreamUrl, {
            cf: { cacheTtl: ttl, cacheEverything: true, cacheKey: upstreamUrl },
        });

        if (!upstream.ok) {
            return new Response(upstream.body, {
                status: upstream.status,
                statusText: upstream.statusText,
                headers: { ...corsHeaders(), 'cache-control': 'no-store' },
            });
        }

        const headers = new Headers(upstream.headers);
        headers.set('content-type',  pickContentType(file, route.defaultContentType));
        headers.set('cache-control', `public, max-age=${ttl}, s-maxage=${ttl}`);
        for (const [k, v] of Object.entries(corsHeaders())) headers.set(k, v);

        return new Response(upstream.body, {
            status: upstream.status,
            statusText: upstream.statusText,
            headers,
        });
    },
};
