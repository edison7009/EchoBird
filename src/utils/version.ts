/** Returns true only if `remote` version is strictly greater than `local` (semver X.Y.Z).
 *  Returns false when either side fails to parse — avoids false "update available" prompts. */
export function isNewerVersion(remote: string, local: string): boolean {
    const parse = (v: string) => v.replace(/^v/, '').split('.').map(Number);
    const r = parse(remote);
    const l = parse(local);
    if (r.length < 3 || l.length < 3 || r.some(isNaN) || l.some(isNaN)) return false;
    const [rMaj, rMin, rPat] = r;
    const [lMaj, lMin, lPat] = l;
    if (rMaj !== lMaj) return rMaj > lMaj;
    if (rMin !== lMin) return rMin > lMin;
    return rPat > lPat;
}
