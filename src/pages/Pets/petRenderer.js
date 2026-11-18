// Pets and supply cards share one initialized WASM instance, including when
// their first requests overlap. Failed loads can be retried by either view.
let loading;
export function loadPetRenderer() {
  if (!loading) {
    loading = import(
      /* @vite-ignore */ new URL(
        'pet-renderer/renderer.js',
        new URL(import.meta.env.BASE_URL, document.baseURI)
      ).href
    )
      .then(async (module) => {
        await module.default();
        return module;
      })
      .catch((error) => {
        loading = undefined;
        throw error;
      });
  }
  return loading;
}
