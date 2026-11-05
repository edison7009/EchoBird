/** Runs an explicit account action. Provider adapters retain native write and launch ordering. */
export async function runAccountApplication(
  apply: () => Promise<void>,
  lifecycle: { current: () => boolean; report: (error: unknown) => void; finish: () => void }
) {
  try {
    await apply();
  } catch (error) {
    if (lifecycle.current()) lifecycle.report(error);
  } finally {
    lifecycle.finish();
  }
}
