// Most między oknami a procesem głównym – tylko wąskie, jawnie nazwane funkcje.
'use strict';

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('granDesktop', {
  /** Wersja programu, limit i folder danych map. */
  info: () => ipcRenderer.invoke('gran:info'),
  /** Okno „Dane map” (to samo co przy pierwszym uruchomieniu). */
  openSettings: () => ipcRenderer.invoke('gran:open-settings'),
  /** Sprawdzenie, czy na redzikowski.com jest nowsza wersja (propozycja aktualizacji pojawia się sama). */
  checkUpdates: () => ipcRenderer.invoke('gran:update:check'),
});

contextBridge.exposeInMainWorld('granSetup', {
  get: () => ipcRenderer.invoke('gran:setup:get'),
  free: (dir) => ipcRenderer.invoke('gran:setup:free', dir),
  pick: () => ipcRenderer.invoke('gran:setup:pick'),
  save: (cfg) => ipcRenderer.invoke('gran:setup:save', cfg),
  clear: () => ipcRenderer.invoke('gran:setup:clear'),
  cancel: () => ipcRenderer.invoke('gran:setup:cancel'),
  checkUpdates: () => ipcRenderer.invoke('gran:update:check'),
});
