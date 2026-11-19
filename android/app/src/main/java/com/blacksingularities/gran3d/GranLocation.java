package com.blacksingularities.gran3d;

import android.Manifest;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.hardware.GeomagneticField;
import android.hardware.Sensor;
import android.hardware.SensorEvent;
import android.hardware.SensorEventListener;
import android.hardware.SensorManager;
import android.location.Location;
import android.location.LocationListener;
import android.location.LocationManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Looper;
import android.view.Surface;
import android.view.WindowManager;

import java.io.FileWriter;
import java.io.IOException;
import java.text.NumberFormat;
import java.util.ArrayList;
import java.util.Locale;

/**
 * Położenie dla GRANI: odczyty z odbiornika GPS (LocationManager – działa też bez usług Google,
 * np. na telefonach Huawei) i kierunek z kompasu. Program odbiera je co klatkę przez
 * {@link #poll()} jako wiersze tekstu (format w {@code src/gps.rs}).
 *
 * <p>Podczas nagrywania każdy odczyt z satelitów trafia od razu do dziennika na dysku, a usługa
 * pierwszoplanowa ({@link GranLocationService}) z powiadomieniem trzyma proces przy życiu przy
 * wyłączonym ekranie i w tle – program po powrocie dostaje wszystkie odczyty z kolejki, a po
 * zamknięciu odtwarza przejście z dziennika.
 */
final class GranLocation implements LocationListener, SensorEventListener {
    private static GranLocation instance;

    private final Context app;
    private final LocationManager lm;
    private final SensorManager sm;
    private final ArrayList<String> queue = new ArrayList<>();
    private boolean running;
    private float heading = Float.NaN;
    private boolean headingNew;
    private FileWriter journal;
    private Location lastFix;
    private String title = "GRAŃ";

    // podsumowanie do powiadomienia nagrywania
    private double distance;
    private Location lastForDistance;
    private long startedAt;
    private long notifiedAt;

    static synchronized GranLocation get(Context c) {
        if (instance == null) instance = new GranLocation(c.getApplicationContext());
        return instance;
    }

    private GranLocation(Context app) {
        this.app = app;
        lm = (LocationManager) app.getSystemService(Context.LOCATION_SERVICE);
        sm = (SensorManager) app.getSystemService(Context.SENSOR_SERVICE);
    }

    static boolean granted(Context c) {
        return c.checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED
                || c.checkSelfPermission(Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED;
    }

    /**
     * Włącza odczyty; niepusty {@code journalPath} – nagrywanie (dziennik i usługa w tle), pusty –
     * samo położenie na mapie. Pusty wynik – sukces.
     */
    synchronized String start(String journalPath, String notificationTitle) {
        if (!granted(app)) return "perm";
        if (lm == null) return "no-gps";
        if (notificationTitle != null && !notificationTitle.isEmpty()) title = notificationTitle;
        if (!running) {
            try {
                if (lm.getAllProviders().contains(LocationManager.GPS_PROVIDER)) {
                    lm.requestLocationUpdates(LocationManager.GPS_PROVIDER, 1000L, 0f, this, Looper.getMainLooper());
                }
                // szybki, przybliżony pierwszy odczyt z sieci (do nagrania trafiają tylko odczyty z satelitów)
                if (lm.getAllProviders().contains(LocationManager.NETWORK_PROVIDER)) {
                    lm.requestLocationUpdates(LocationManager.NETWORK_PROVIDER, 3000L, 0f, this, Looper.getMainLooper());
                }
                Location last = null;
                for (String p : new String[]{LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER}) {
                    Location l = lm.getLastKnownLocation(p);
                    if (l != null && (last == null || l.getTime() > last.getTime())) last = l;
                }
                // ostatnie znane położenie sprzed najwyżej 2 minut – kropka od razu na mapie
                if (last != null && System.currentTimeMillis() - last.getTime() < 120_000) push(last, false);
            } catch (SecurityException e) {
                return "perm";
            } catch (IllegalArgumentException e) {
                return "no-gps";
            }
            if (sm != null) {
                Sensor rot = sm.getDefaultSensor(Sensor.TYPE_ROTATION_VECTOR);
                if (rot != null) sm.registerListener(this, rot, SensorManager.SENSOR_DELAY_UI);
            }
            running = true;
        }
        boolean record = journalPath != null && !journalPath.isEmpty();
        if (record && journal == null) {
            try {
                journal = new FileWriter(journalPath, true);
            } catch (IOException e) {
                return "journal: " + e.getMessage();
            }
            distance = 0;
            lastForDistance = null;
            startedAt = System.currentTimeMillis();
            Intent i = new Intent(app, GranLocationService.class);
            i.putExtra("title", title);
            i.putExtra("text", summary());
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) app.startForegroundService(i);
            else app.startService(i);
        } else if (!record && journal != null) {
            closeJournal();
        }
        return "";
    }

    synchronized void stop() {
        if (running) {
            lm.removeUpdates(this);
            if (sm != null) sm.unregisterListener(this);
            running = false;
        }
        closeJournal();
        heading = Float.NaN;
    }

    private void closeJournal() {
        if (journal != null) {
            try {
                journal.close();
            } catch (IOException ignored) {
            }
            journal = null;
        }
        app.stopService(new Intent(app, GranLocationService.class));
    }

    /** Odczyty od ostatniego wywołania; pierwszy wiersz „H TAB stopnie” – kompas. */
    synchronized String poll() {
        StringBuilder sb = new StringBuilder();
        if (headingNew && !Float.isNaN(heading)) {
            sb.append("H\t").append(String.format(Locale.ROOT, "%.1f", heading)).append('\n');
            headingNew = false;
        }
        for (String l : queue) sb.append(l).append('\n');
        queue.clear();
        return sb.toString();
    }

    private static String num(boolean has, double v, String fmt) {
        return has ? String.format(Locale.ROOT, fmt, v) : "";
    }

    private synchronized void push(Location l, boolean live) {
        boolean gnss = LocationManager.GPS_PROVIDER.equals(l.getProvider());
        boolean vacc = Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && l.hasVerticalAccuracy();
        String line = String.format(Locale.ROOT, "%.3f,%.7f,%.7f,%s,%.1f,%s,%s,%s,%s",
                l.getTime() / 1000.0, l.getLatitude(), l.getLongitude(),
                num(l.hasAltitude(), l.getAltitude(), "%.2f"),
                l.hasAccuracy() ? l.getAccuracy() : 99f,
                vacc ? String.format(Locale.ROOT, "%.2f", l.getVerticalAccuracyMeters()) : "",
                num(l.hasSpeed(), l.getSpeed(), "%.2f"),
                num(l.hasBearing(), l.getBearing(), "%.1f"),
                gnss ? "g" : "n");
        // bez programu na pierwszym planie kolejka rośnie (ok. 1 wiersz na sekundę) – górny limit na wiele godzin
        if (queue.size() > 200_000) queue.subList(0, 20_000).clear();
        queue.add(line);
        if (gnss || lastFix == null) lastFix = l;
        if (live && gnss && journal != null) {
            try {
                journal.write(line);
                journal.write('\n');
                journal.flush();
            } catch (IOException ignored) {
            }
            if (l.hasAccuracy() && l.getAccuracy() <= 30f) {
                if (lastForDistance != null) {
                    float d = lastForDistance.distanceTo(l);
                    if (d >= Math.max(3f, l.getAccuracy() * 0.6f)) {
                        distance += d;
                        lastForDistance = l;
                    }
                } else {
                    lastForDistance = l;
                }
            }
            long now = System.currentTimeMillis();
            if (now - notifiedAt > 10_000) {
                notifiedAt = now;
                GranLocationService.update(app, title, summary());
            }
        }
    }

    /** „2,4 km · 0:45” – do powiadomienia (dystans przybliżony, dokładne liczy program). */
    private String summary() {
        long s = Math.max(0, (System.currentTimeMillis() - startedAt) / 1000);
        NumberFormat nf = NumberFormat.getNumberInstance(Locale.getDefault());
        nf.setMinimumFractionDigits(distance < 10_000 ? 2 : 1);
        nf.setMaximumFractionDigits(distance < 10_000 ? 2 : 1);
        return nf.format(distance / 1000.0) + " km · " + String.format(Locale.ROOT, "%d:%02d", s / 3600, (s % 3600) / 60);
    }

    // ---------------------------------------------------------------- LocationListener

    @Override
    public void onLocationChanged(Location l) {
        push(l, true);
    }

    @Override
    public void onStatusChanged(String provider, int status, Bundle extras) {
    }

    @Override
    public void onProviderEnabled(String provider) {
    }

    @Override
    public void onProviderDisabled(String provider) {
    }

    // ---------------------------------------------------------------- kompas

    @Override
    public void onSensorChanged(SensorEvent e) {
        float[] r = new float[9];
        float[] out = new float[9];
        SensorManager.getRotationMatrixFromVector(r, e.values);
        // obrót ekranu: kierunek góry ekranu, nie osi urządzenia
        int rot = Surface.ROTATION_0;
        try {
            WindowManager wm = (WindowManager) app.getSystemService(Context.WINDOW_SERVICE);
            if (wm != null) rot = wm.getDefaultDisplay().getRotation();
        } catch (Exception ignored) {
        }
        int ax = SensorManager.AXIS_X, ay = SensorManager.AXIS_Y;
        switch (rot) {
            case Surface.ROTATION_90: ax = SensorManager.AXIS_Y; ay = SensorManager.AXIS_MINUS_X; break;
            case Surface.ROTATION_180: ax = SensorManager.AXIS_MINUS_X; ay = SensorManager.AXIS_MINUS_Y; break;
            case Surface.ROTATION_270: ax = SensorManager.AXIS_MINUS_Y; ay = SensorManager.AXIS_X; break;
            default: break;
        }
        SensorManager.remapCoordinateSystem(r, ax, ay, out);
        float[] o = new float[3];
        SensorManager.getOrientation(out, o);
        float az = (float) Math.toDegrees(o[0]);
        // północ geograficzna zamiast magnetycznej (deklinacja w miejscu ostatniego odczytu)
        Location l = lastFix;
        if (l != null) {
            az += new GeomagneticField((float) l.getLatitude(), (float) l.getLongitude(), (float) l.getAltitude(), System.currentTimeMillis()).getDeclination();
        }
        az = (az % 360f + 360f) % 360f;
        synchronized (this) {
            heading = az;
            headingNew = true;
        }
    }

    @Override
    public void onAccuracyChanged(Sensor sensor, int accuracy) {
    }
}
