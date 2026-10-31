// App-wide live state: the controller's state polled every few seconds, settings, and a
// one-second clock for countdowns.

import type { Settings, SettingsPatch, StateResponse } from '@aircon/api';

import { api } from './api';

const POLL_MS = 5000;

class Live {
  state = $state<StateResponse | null>(null);
  settings = $state<Settings | null>(null);
  /** Set when the API itself can't be reached (as opposed to the AirTouch). */
  error = $state<string | null>(null);
  /** Last action error, shown as a toast. */
  notice = $state<string | null>(null);
  /** Unix seconds, ticking once a second. */
  now = $state(Date.now() / 1000);

  #started = false;
  #noticeTimer: ReturnType<typeof setTimeout> | undefined;

  start() {
    if (this.#started) return;
    this.#started = true;
    void this.refresh();
    void this.loadSettings();
    setInterval(() => void this.refresh(), POLL_MS);
    setInterval(() => (this.now = Date.now() / 1000), 1000);
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible') void this.refresh();
    });
  }

  async refresh() {
    if (document.visibilityState === 'hidden' && this.state) return;
    try {
      this.state = await api.state();
      this.error = null;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    }
  }

  async loadSettings() {
    try {
      this.settings = await api.settings();
    } catch {
      // The state poll reports connectivity; settings retry on the next change.
    }
  }

  /**
   * Run an action, then re-check right away so the UI shows its effect. A second refresh a
   * moment later picks up the AirTouch's own status push for zone changes.
   */
  async act(action: () => Promise<unknown>): Promise<boolean> {
    try {
      await action();
    } catch (e) {
      this.flash(e instanceof Error ? e.message : String(e));
      return false;
    }
    try {
      this.state = await api.tick();
    } catch {
      // The action went through; the next poll catches up.
    }
    setTimeout(() => void this.refresh(), 1500);
    return true;
  }

  async patchSettings(patch: SettingsPatch) {
    try {
      this.settings = await api.patchSettings(patch);
      this.state = await api.tick();
    } catch (e) {
      this.flash(e instanceof Error ? e.message : String(e));
      await this.loadSettings();
    }
  }

  flash(message: string) {
    this.notice = message;
    clearTimeout(this.#noticeTimer);
    this.#noticeTimer = setTimeout(() => (this.notice = null), 5000);
  }
}

export const live = new Live();
