import type { ControllerState, Settings, StateResponse, ZoneState } from '@aircon/api';
import { describe, expect, test } from 'vite-plus/test';

import { hero } from './hero';

function zone(number: number, name: string, temperature: number, on = true): ZoneState {
  return {
    number,
    name,
    on,
    control: 'percentage',
    setpoint: 24,
    temperature,
    damper: 0.5,
    spill: false,
    has_sensor: true,
    low_battery: false,
    target: 24,
    error: on ? Math.round((temperature - 24) * 100) / 100 : null,
    active: on,
    driving: false,
  };
}

function state(
  controller: Partial<ControllerState>,
  zones = [zone(3, 'Study', 26.9)],
): StateResponse {
  return {
    now: 0,
    build: { version: 'dev', commit: null },
    airtouch: {
      host: 'x',
      connected: true,
      last_packet_at: 0,
      last_error: null,
      console_version: '1.3.3',
    },
    ac: {
      on: true,
      mode: 'auto_cool',
      direction: 'cool',
      fan: 'low',
      setpoint: 24,
      temperature: 24,
      spill: false,
      error_code: 0,
      fans: ['low', 'medium', 'high'],
      limits: { cool: [18, 25], heat: [16, 26] },
      power_since: 0,
    },
    zones,
    controller: {
      status: 'running',
      reason: '',
      dry_run: false,
      command: null,
      worst: null,
      hold_seconds: 0,
      enabled: true,
      parked: false,
      paused_until: null,
      pause_reason: null,
      next_tick_at: null,
      tick_seconds: 60,
      ...controller,
    },
  };
}

const worst = { number: 3, name: 'Study', active: true, target: 24, error: 2.9 };
const command = { power: null, fan: 'high' as const, setpoint: 21, text: 'fan high, setpoint 21°' };

describe('hero', () => {
  test('running live', () => {
    const h = hero(state({ worst, command }), null);
    expect(h).toMatchObject({
      tone: 'cool',
      pill: 'Cooling',
      headline: 'Study is 2.9° above target',
      sub: 'Fan Low → High · AC setpoint 21.0°',
    });
  });

  test('running in dry run says what it would do', () => {
    const h = hero(state({ worst, command, dry_run: true }), null);
    expect(h.headline).toBe('Would set fan High · AC setpoint 21.0°');
    expect(h.sub).toBe('Study is 2.9° above target · nothing sent to the AC');
  });

  test('idle names the resume temperature', () => {
    const settings = { deadband: 2 } as Settings;
    const h = hero(
      state({ status: 'idle', hold_seconds: 120 }, [zone(3, 'Study', 23.8)]),
      settings,
    );
    expect(h).toMatchObject({
      headline: 'All rooms at target',
      sub: 'Resumes when a room reaches 26.0°',
      hold: 120,
    });
  });

  test('paused after a manual change', () => {
    const h = hero(
      state({
        status: 'paused',
        pause_reason: 'Manual change: fan low → high',
        paused_until: null,
      }),
      null,
    );
    expect(h.headline).toBe('Manual change in AirTouch app');
    expect(h.sub).toBe('fan low → high');
    expect(h.actions).toEqual(['resume', 'extend']);
  });

  test('no rooms on', () => {
    expect(hero(state({ status: 'standby', reason: 'No rooms on' }), null).headline).toBe(
      'All rooms are off',
    );
  });

  test('disconnected is an error', () => {
    expect(hero(state({ status: 'disconnected' }), null)).toMatchObject({
      tone: 'error',
      headline: "Can't reach AirTouch",
    });
  });
});
