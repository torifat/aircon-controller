// Hash routes, so the Rust server only ever has to serve index.html.

export type Route = 'home' | 'history' | 'activity' | 'settings';

export const routes: { id: Route; label: string; href: string }[] = [
  { id: 'home', label: 'Home', href: '#/' },
  { id: 'history', label: 'History', href: '#/history' },
  { id: 'activity', label: 'Activity', href: '#/activity' },
  { id: 'settings', label: 'Settings', href: '#/settings' },
];

function parse(hash: string): Route {
  const id = hash.replace(/^#\/?/, '').split(/[/?]/)[0];
  return routes.some((r) => r.id === id) ? (id as Route) : 'home';
}

class Router {
  route = $state<Route>(parse(location.hash));

  constructor() {
    addEventListener('hashchange', () => {
      this.route = parse(location.hash);
      scrollTo({ top: 0 });
    });
  }
}

export const router = new Router();
