// Most między oknami a procesem głównym – tylko wąskie, jawnie nazwane funkcje.
'use strict';

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('granDesktop', {
  /** Wersja programu, tryb przechowywania danych, limit i katalog danych. */
  info: () => ipcRenderer.invoke('gran:info'),
  /** Okno „Dane i pamięć” (to samo co przy pierwszym uruchomieniu). */
  openSettings: () => ipcRenderer.invoke('gran:open-settings'),
});

contextBridge.exposeInMainWorld('granSetup', {
  get: () => ipcRenderer.invoke('gran:setup:get'),
  free: (dir) => ipcRenderer.invoke('gran:setup:free', dir),
  pick: () => ipcRenderer.invoke('gran:setup:pick'),
  save: (cfg) => ipcRenderer.invoke('gran:setup:save', cfg),
  clear: () => ipcRenderer.invoke('gran:setup:clear'),
  cancel: () => ipcRenderer.invoke('gran:setup:cancel'),
});
