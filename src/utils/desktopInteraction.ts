/** Suppress WebView page menus while retaining native text editing menus. */
export function handleDesktopContextMenu(event: MouseEvent): void {
  const target = event.target;
  if (target instanceof HTMLElement) {
    const field = target.closest('input, textarea');
    if (
      field &&
      !field.matches(
        ':disabled, [type="button"], [type="submit"], [type="reset"], [type="checkbox"], [type="radio"], [type="range"], [type="color"], [type="file"], [type="image"], [type="hidden"]'
      )
    )
      return;
    if (target.isContentEditable) return;
  }
  event.preventDefault();
}
