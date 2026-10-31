import { describe, expect, test } from 'vite-plus/test';

import { linePath, nearest, runs, yRange } from './chart';

describe('chart', () => {
  test('runs merge equal neighbours and split on gaps', () => {
    const pts = [
      { ts: 0, on: true },
      { ts: 60, on: true },
      { ts: 120, on: false },
      { ts: 180, on: true },
      { ts: 1000, on: true },
    ];
    expect(runs(pts, (p) => (p.on ? 'on' : null), 120)).toEqual([
      { from: 0, to: 120, value: 'on' },
      { from: 180, to: 240, value: 'on' },
      { from: 1000, to: 1060, value: 'on' },
    ]);
  });

  test('linePath breaks across gaps', () => {
    const pts = [
      { ts: 0, v: 1 },
      { ts: 60, v: 2 },
      { ts: 600, v: 3 },
    ];
    expect(
      linePath(
        pts,
        (t) => t,
        (v) => v,
        120,
      ),
    ).toBe('M0.0 1.0 L60.0 2.0 M600.0 3.0');
  });

  test('yRange', () => {
    expect(yRange([23.9, 26.9])).toEqual([23, 28]);
    expect(yRange([24, 24.2])).toEqual([22, 26]);
  });

  test('nearest', () => {
    expect(nearest([{ ts: 0 }, { ts: 60 }, { ts: 120 }], 80)).toEqual({ ts: 60 });
  });
});
