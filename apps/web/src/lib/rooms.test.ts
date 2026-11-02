import { describe, expect, test } from 'vite-plus/test';

import { barPos, delta, eta, need, spark, sparkY, trend } from './rooms';

describe('rooms', () => {
  test('need', () => {
    expect(need(0.9, false)).toBe(0.9);
    expect(need(0.9, true)).toBe(-0.9);
  });

  test('delta', () => {
    expect(delta(0.3)).toEqual({ text: 'on target', words: 'on target', tone: 'mu' });
    expect(delta(0.9)).toEqual({ text: '+0.9°', words: '0.9° above target', tone: 'warm' });
    expect(delta(-1.2)).toEqual({ text: '−1.2°', words: '1.2° below target', tone: 'cool' });
  });

  test('barPos', () => {
    expect(barPos(0)).toBe(50);
    expect(barPos(1.5)).toBe(74);
    expect(barPos(-9)).toBe(2);
  });

  test('sparkY', () => {
    expect(sparkY(24, 24)).toBe(18);
    expect(sparkY(27, 24)).toBe(2);
    expect(sparkY(10, 24)).toBe(34);
  });

  test('spark', () => {
    const now = 7200;
    const samples = [0, 1800, 3600, 5400, 7200].map((ts, i) => ({ ts, v: 26 - i * 0.25 }));
    const s = spark(samples, 24, now, 1800);
    expect(s.d.startsWith('M0.0 ')).toBe(true);
    expect(s.d.split('L')).toHaveLength(5);
    expect(s.endY).toBeCloseTo(sparkY(25, 24));
    expect(s.rate).toBeCloseTo(-0.5);
    // Ten minutes of readings isn't enough to call a trend.
    expect(spark(samples.slice(-1), 24, now, 1800).rate).toBeNull();
  });

  test('eta', () => {
    expect(eta(0.01)).toBe('10m');
    expect(eta(0.66)).toBe('40m');
    expect(eta(1.33)).toBe('1h 20m');
    expect(eta(2)).toBe('2h');
  });

  test('trend', () => {
    expect(trend(1, null).text).toBe('Collecting…');
    expect(trend(0.2, 0).text).toBe('Holding at target');
    expect(trend(0.2, 0.3).text).toBe('Near target');
    expect(trend(0.9, -0.6)).toMatchObject({ text: 'Closing 0.6°/h · ~1h 30m', tone: 'mu' });
    expect(trend(-0.9, 0.6).text).toBe('Closing 0.6°/h · ~1h 30m');
    expect(trend(0.9, 0.3)).toMatchObject({
      text: 'Drifting away 0.3°/h',
      tone: 'warm',
      strong: true,
    });
    expect(trend(-0.9, 0.01)).toMatchObject({ text: 'Not moving', tone: 'cool', strong: true });
  });
});
