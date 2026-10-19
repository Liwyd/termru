/**
 * Pure terminal-fitting math, unit-tested independently of xterm/DOM.
 * The xterm view (Phase 5) will call this against measured cell metrics.
 */
export interface FitResult {
  cols: number;
  rows: number;
}

export function computeFit(
  containerWidth: number,
  containerHeight: number,
  cellWidth: number,
  cellHeight: number,
): FitResult {
  if (containerWidth <= 0 || containerHeight <= 0 || cellWidth <= 0 || cellHeight <= 0) {
    return { cols: 0, rows: 0 };
  }
  return {
    cols: Math.max(2, Math.floor(containerWidth / cellWidth)),
    rows: Math.max(1, Math.floor(containerHeight / cellHeight)),
  };
}
