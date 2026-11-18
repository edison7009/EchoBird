/** Ignore translucent shadow pixels when measuring the visible body of a native icon. */
export function iconContentBounds(pixels: Uint8ClampedArray, width: number, height: number) {
  let left = width;
  let top = height;
  let right = -1;
  let bottom = -1;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (pixels[(y * width + x) * 4 + 3] < 128) continue;
      left = Math.min(left, x);
      top = Math.min(top, y);
      right = Math.max(right, x);
      bottom = Math.max(bottom, y);
    }
  }
  return right < left
    ? null
    : { x: left, y: top, width: right - left + 1, height: bottom - top + 1 };
}

/** Fit native icon artwork to the existing tile, rather than enlarging its layout box. */
export function fitSystemApplicationIcon(src: string, image: HTMLImageElement): string {
  if (typeof document === 'undefined' || !image.naturalWidth || !image.naturalHeight) return src;
  try {
    const ratio = Math.min(1, 256 / Math.max(image.naturalWidth, image.naturalHeight));
    const width = Math.max(1, Math.round(image.naturalWidth * ratio));
    const height = Math.max(1, Math.round(image.naturalHeight * ratio));
    const input = document.createElement('canvas');
    input.width = width;
    input.height = height;
    const context = input.getContext('2d', { willReadFrequently: true });
    if (!context) return src;
    context.drawImage(image, 0, 0, width, height);
    const bounds = iconContentBounds(context.getImageData(0, 0, width, height).data, width, height);
    if (!bounds || Math.max(bounds.width / width, bounds.height / height) >= 0.95) return src;
    // Retain a small fringe for antialiasing/shadows and preserve the aspect ratio.
    const fringe = Math.max(1, Math.ceil(Math.max(width, height) / 64));
    const x = Math.max(0, bounds.x - fringe);
    const y = Math.max(0, bounds.y - fringe);
    const cropWidth = Math.min(width, bounds.x + bounds.width + fringe) - x;
    const cropHeight = Math.min(height, bounds.y + bounds.height + fringe) - y;
    const output = document.createElement('canvas');
    output.width = width;
    output.height = height;
    const target = output.getContext('2d');
    if (!target) return src;
    const scale = Math.min(width / cropWidth, height / cropHeight);
    const fittedWidth = cropWidth * scale;
    const fittedHeight = cropHeight * scale;
    target.drawImage(
      input,
      x,
      y,
      cropWidth,
      cropHeight,
      (width - fittedWidth) / 2,
      (height - fittedHeight) / 2,
      fittedWidth,
      fittedHeight
    );
    const fitted = output.toDataURL('image/png');
    return fitted.startsWith('data:image/png;base64,') ? fitted : src;
  } catch {
    // Keep the original icon if the webview cannot read or export a canvas.
    return src;
  }
}
