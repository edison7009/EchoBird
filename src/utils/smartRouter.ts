export function isSmartRouterId(id: string | null | undefined): boolean {
  return id === 'smart-router' || id?.startsWith('smart-router-') === true;
}
