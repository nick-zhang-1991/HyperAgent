// Single source of truth for backend URL. Desktop (Tauri) always points
// to the local backend on 127.0.0.1:4000 — using a relative path would
// hit the static preview/webview origin and 404. Web (Vite dev / reverse
// proxy) keeps the relative default so a reverse proxy can route /api/*.
function isTauri() {
    return typeof window !== 'undefined' &&
        (window.__TAURI_INTERNALS__ !== undefined || window.__TAURI__ !== undefined);
}
const DEFAULT_BACKEND = 'http://127.0.0.1:4000';
// Allow override via localStorage (set from Settings if we ever add that)
// or via Vite env (VITE_API_BASE) at build time.
const buildOverride = (typeof import.meta !== 'undefined' &&
    import.meta.env?.VITE_API_BASE) || '';
export function apiBase() {
    if (!isTauri())
        return buildOverride || ''; // web: keep relative for proxy
    if (typeof localStorage !== 'undefined') {
        const ls = localStorage.getItem('apiBase');
        if (ls)
            return ls;
    }
    return buildOverride || DEFAULT_BACKEND;
}
export function wsBase() {
    const http = apiBase();
    if (!http) {
        if (typeof window !== 'undefined') {
            const proto = window.location.protocol === 'https:' ? 'wss' : 'ws';
            return `${proto}://${window.location.host}`;
        }
        return '';
    }
    return http.replace(/^http/, 'ws');
}
export async function apiFetch(path, init) {
    return fetch(apiBase() + path, init);
}
