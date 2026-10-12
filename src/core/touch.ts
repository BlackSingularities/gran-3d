// Urządzenia dotykowe (telefony, tablety): teksty podpowiedzi i drobne różnice w interfejsie.
export const TOUCH = typeof matchMedia !== 'undefined' && matchMedia('(hover: none) and (pointer: coarse)').matches;

/** „Kliknij” / „Dotknij” – z wielką literą. */
export const CLICK = TOUCH ? 'Dotknij' : 'Kliknij';
/** „kliknij” / „dotknij” – małą literą. */
export const click = TOUCH ? 'dotknij' : 'kliknij';
/** gest menu analiz */
export const CTX_GESTURE = TOUCH ? 'Przytrzymaj palec na terenie' : 'Prawy klik na terenie';
