// OTOKO : permet d'installer la page comme une appli. Pas de hors-ligne : tout passe par le réseau.
self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', e => e.waitUntil(self.clients.claim()));
