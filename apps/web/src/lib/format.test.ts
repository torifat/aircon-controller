import { describe, expect, test } from 'vite-plus/test';

import { ago, countdown, dayLabel, deg, duration, gapChip, modeName } from './format';

describe('format', () => {
  test('deg', () => {
    expect(deg(24)).toBe('24.0°');
    expect(deg(null)).toBe('—');
  });

  test('duration', () => {
    expect(duration(45)).toBe('45s');
    expect(duration(12 * 60)).toBe('12m');
    expect(duration(97 * 60)).toBe('1h 37m');
    expect(duration(120 * 60)).toBe('2h');
  });

  test('countdown', () => {
    expect(countdown(38)).toBe('0:38');
    expect(countdown(245)).toBe('4:05');
    expect(countdown(3729)).toBe('1:02:09');
    expect(countdown(-5)).toBe('0:00');
  });

  test('ago', () => {
    expect(ago(96, 100)).toBe('4 s ago');
    expect(ago(100 - 180, 100)).toBe('3 min ago');
    expect(ago(null, 100)).toBe('never');
  });

  test('dayLabel', () => {
    const now = new Date(2026, 9, 31, 14, 0).getTime() / 1000;
    expect(dayLabel(now - 3600, now)).toBe('Today');
    expect(dayLabel(now - 20 * 3600, now)).toBe('Yesterday');
  });

  test('modeName', () => {
    expect(modeName('auto_cool')).toBe('Auto');
    expect(modeName('cool')).toBe('Cool');
  });

  test('gapChip', () => {
    expect(gapChip(0.3)).toEqual({ text: 'On target', tone: 'none', far: false });
    expect(gapChip(2.9)).toEqual({ text: '2.9° too warm', tone: 'warm', far: true });
    expect(gapChip(-0.8)).toEqual({ text: '0.8° too cool', tone: 'cool', far: false });
  });
});
