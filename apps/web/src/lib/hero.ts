// What the top of Home says: the controller's state in plain words.

import type { CommandView, Settings, StateResponse } from '@aircon/api';

import { capitalize, clock, deg } from './format';

export type Tone = 'cool' | 'warm' | 'neutral' | 'error';

export type Hero = {
  tone: Tone;
  pill: string;
  headline: string;
  sub: string;
  /** Seconds left on a compressor min on/off time, if one is holding a decision. */
  hold: number;
  holdLabel: string;
  actions: ('resume' | 'extend' | 'enable')[];
};

function gapText(name: string, error: number, heating: boolean): string {
  const above = heating ? error < 0 : error >= 0;
  return `${name} is ${Math.abs(error).toFixed(1)}° ${above ? 'above' : 'below'} target`;
}

/** Dry-run phrasing: "Would set fan High · AC setpoint 21.0°", "Would turn the AC off". */
function would(cmd: CommandView): string {
  const parts: string[] = [];
  if (cmd.power != null) parts.push(cmd.power ? 'turn the AC on' : 'turn the AC off');
  if (cmd.fan) parts.push(`set fan ${capitalize(cmd.fan)}`);
  if (cmd.setpoint != null) parts.push(`AC setpoint ${deg(cmd.setpoint)}`);
  return `Would ${parts.join(' · ')}`;
}

export function hero(s: StateResponse, settings: Settings | null): Hero {
  const c = s.controller;
  const ac = s.ac;
  const heating = ac?.direction === 'heat';
  const base: Hero = {
    tone: 'neutral',
    pill: capitalize(c.status),
    headline: c.reason,
    sub: '',
    hold: c.hold_seconds,
    holdLabel: '',
    actions: [],
  };

  switch (c.status) {
    case 'starting':
      return { ...base, pill: 'Starting', headline: 'Connecting to AirTouch' };
    case 'disconnected':
      return {
        ...base,
        tone: 'error',
        pill: 'Offline',
        headline: "Can't reach AirTouch",
        sub: 'No changes are being sent',
      };
    case 'error':
      return {
        ...base,
        tone: 'error',
        pill: 'AC error',
        headline: `AC reports error ${ac?.error_code ?? ''}`.trim(),
        sub: 'Climate has stopped adjusting. Check the Daikin controller.',
      };
    case 'disabled':
      return {
        ...base,
        pill: 'Controller off',
        headline: 'Controller off',
        sub: 'AirTouch is running on its own',
        actions: ['enable'],
      };
    case 'paused': {
      const until = c.paused_until ? `resumes ${clock(c.paused_until)}` : '';
      const manual = c.pause_reason?.startsWith('Manual change: ');
      return {
        ...base,
        pill: 'Paused',
        headline: manual ? 'Manual change in AirTouch app' : 'Paused',
        sub: [manual ? c.pause_reason!.slice('Manual change: '.length) : c.pause_reason, until]
          .filter(Boolean)
          .join(' · '),
        actions: ['resume', 'extend'],
      };
    }
    case 'off':
      return {
        ...base,
        pill: 'Off',
        headline: 'AC is off',
        sub: 'Climate takes over when the AC is turned on',
      };
    case 'standby':
      return c.reason === 'No rooms on'
        ? {
            ...base,
            pill: 'Standby',
            headline: 'All rooms are off',
            sub: 'Turn a room on to start',
          }
        : { ...base, pill: 'Standby', headline: c.reason };
    case 'idle': {
      const deadband = settings?.deadband ?? 2;
      const targets = s.zones.flatMap((z) => (z.active && z.target != null ? [z.target] : []));
      // With one shared target there's a single temperature to name; otherwise say the rule.
      const sub = !targets.length
        ? ''
        : new Set(targets).size === 1
          ? `Resumes when a room reaches ${deg(targets[0] + (heating ? -deadband : deadband))}`
          : `Resumes when a room is ${deadband}° past target`;
      return {
        ...base,
        pill: 'Idle',
        headline: 'All rooms at target',
        sub,
        holdLabel: 'can restart in',
      };
    }
    case 'running': {
      const tone: Tone = heating ? 'warm' : 'cool';
      const pill = heating ? 'Heating' : 'Cooling';
      const w = c.worst;
      const gap = w?.error != null ? gapText(w.name, w.error, heating) : c.reason;
      const cmd = c.command;
      if (w?.error != null && w.error <= 0) {
        return {
          ...base,
          tone,
          pill,
          headline: 'All rooms at target',
          sub: 'Keeping the AC on for its minimum run time',
          holdLabel: 'turns off in',
        };
      }
      if (c.dry_run && cmd) {
        return {
          ...base,
          tone,
          pill,
          headline: would(cmd),
          sub: `${gap} · nothing sent to the AC`,
        };
      }
      const fan = ac ? capitalize(ac.fan) : '';
      const fanText =
        cmd?.fan && ac && cmd.fan !== ac.fan ? `Fan ${fan} → ${capitalize(cmd.fan)}` : `Fan ${fan}`;
      const sp = cmd?.setpoint ?? ac?.setpoint;
      return { ...base, tone, pill, headline: gap, sub: `${fanText} · AC setpoint ${deg(sp)}` };
    }
  }
}
