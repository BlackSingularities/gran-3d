package com.blacksingularities.gran3d;

import android.content.ContentValues;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.MediaStore;
import android.provider.Settings;
import android.util.Base64;
import android.webkit.JavascriptInterface;
import android.webkit.WebView;
import android.widget.Toast;

import androidx.activity.OnBackPressedCallback;
import androidx.core.content.FileProvider;

import com.getcapacitor.BridgeActivity;

import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.Locale;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;

/**
 * GRAŃ – główna aktywność. Interfejs atlasu działa w WebView i rozmawia z lokalnym serwerem
 * (Node.js uruchamiany przez wtyczkę capacitor-nodejs na 127.0.0.1:47190).
 * Most „GranAndroid” daje stronie to, czego WebView nie umie sam: zapis plików (GPX, zrzuty)
 * do folderu „Pobrane” i otwieranie linków w przeglądarce systemowej.
 */
public class MainActivity extends BridgeActivity {

    @Override
    public void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        WebView webView = getBridge().getWebView();
        webView.addJavascriptInterface(new GranBridge(), "GranAndroid");
        // pamięć podręczna HTTP WebView: kafle i zdjęcia wczytywane ponownie bez sieci
        webView.getSettings().setCacheMode(android.webkit.WebSettings.LOAD_DEFAULT);
        // układ interfejsu niezależny od systemowej skali czcionek
        webView.getSettings().setTextZoom(100);
        // „Wstecz”: najpierw zamyka okna, panel i tryby w aplikacji, dopiero potem wychodzi
        getOnBackPressedDispatcher().addCallback(this, new OnBackPressedCallback(true) {
            @Override
            public void handleOnBackPressed() {
                final OnBackPressedCallback self = this;
                webView.evaluateJavascript("(window.__granBack && window.__granBack()) ? 'yes' : 'no'", (r) -> {
                    if (r != null && r.contains("yes")) return;
                    self.setEnabled(false);
                    getOnBackPressedDispatcher().onBackPressed();
                    self.setEnabled(true);
                });
            }
        });
    }

    @Override
    public void onDestroy() {
        // tryb „usuwaj za każdym razem”: dane map znikają przy zamknięciu aplikacji
        if (isFinishing() && sessionMode()) {
            final File maps = new File(dataRoot(), "maps");
            new Thread(() -> deleteTree(maps)).start();
            getBridge().getWebView().clearCache(true);
        }
        super.onDestroy();
    }

    private File dataRoot() {
        return new File(new File(getFilesDir(), "nodejs"), "data");
    }

    private boolean sessionMode() {
        try {
            File f = new File(dataRoot(), "settings.json");
            if (!f.exists()) return false;
            String json = new String(Files.readAllBytes(f.toPath()), StandardCharsets.UTF_8);
            return "session".equals(new JSONObject(json).optString("mode"));
        } catch (Exception e) {
            return false;
        }
    }

    private static void deleteTree(File f) {
        File[] children = f.listFiles();
        if (children != null) for (File c : children) deleteTree(c);
        //noinspection ResultOfMethodCallIgnored
        f.delete();
    }

    // ------------------------------------------------------------------ aktualizacje
    private volatile boolean updating = false;
    private File pendingApk = null;

    private void updateProgress(double p) {
        final String js = String.format(Locale.US, "window.__granUpdateProgress && window.__granUpdateProgress(%.3f)", p);
        runOnUiThread(() -> getBridge().getWebView().evaluateJavascript(js, null));
    }

    /** Otwiera systemowy instalator; bez zgody na instalowanie z tej aplikacji – najpierw ustawienia. */
    private void launchInstaller(File apk) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !getPackageManager().canRequestPackageInstalls()) {
            pendingApk = apk;
            Toast.makeText(this, "Zezwól aplikacji GRAŃ na instalowanie aktualizacji, potem wróć.", Toast.LENGTH_LONG).show();
            startActivity(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + getPackageName())));
            return;
        }
        Uri uri = FileProvider.getUriForFile(this, getPackageName() + ".fileprovider", apk);
        Intent i = new Intent(Intent.ACTION_VIEW);
        i.setDataAndType(uri, "application/vnd.android.package-archive");
        i.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_ACTIVITY_NEW_TASK);
        startActivity(i);
        updateProgress(2);
    }

    @Override
    public void onResume() {
        super.onResume();
        // powrót z ustawień „nieznanych aplikacji” – kontynuacja instalacji
        if (pendingApk != null && (Build.VERSION.SDK_INT < Build.VERSION_CODES.O || getPackageManager().canRequestPackageInstalls())) {
            File apk = pendingApk;
            pendingApk = null;
            launchInstaller(apk);
        }
    }

    private void downloadUpdate(String url, String version) {
        if (updating) return;
        updating = true;
        new Thread(() -> {
            File dir = new File(getCacheDir(), "updates");
            //noinspection ResultOfMethodCallIgnored
            dir.mkdirs();
            File apk = new File(dir, "GRAN-" + version.replaceAll("[^0-9A-Za-z.\\-]", "") + ".apk");
            File part = new File(dir, apk.getName() + ".part");
            try {
                if (!apk.exists()) {
                    // stare pobrania
                    File[] old = dir.listFiles();
                    if (old != null) for (File f : old) //noinspection ResultOfMethodCallIgnored
                        f.delete();
                    HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
                    c.setInstanceFollowRedirects(true);
                    c.setConnectTimeout(20000);
                    c.setReadTimeout(60000);
                    c.setRequestProperty("User-Agent", "GRAN-Android/" + version);
                    int code = c.getResponseCode();
                    if (code >= 300) throw new IllegalStateException("HTTP " + code);
                    long total = c.getContentLengthLong();
                    long got = 0, lastReport = 0;
                    try (InputStream in = c.getInputStream(); OutputStream out = new FileOutputStream(part)) {
                        byte[] buf = new byte[1 << 16];
                        int n;
                        while ((n = in.read(buf)) > 0) {
                            out.write(buf, 0, n);
                            got += n;
                            long now = System.currentTimeMillis();
                            if (total > 0 && now - lastReport > 250) {
                                lastReport = now;
                                updateProgress(Math.min(0.99, (double) got / total));
                            }
                        }
                    }
                    if (!part.renameTo(apk)) throw new IllegalStateException("rename");
                }
                updateProgress(1);
                runOnUiThread(() -> launchInstaller(apk));
            } catch (Exception e) {
                //noinspection ResultOfMethodCallIgnored
                part.delete();
                updateProgress(-1);
            } finally {
                updating = false;
            }
        }).start();
    }

    private class GranBridge {
        @JavascriptInterface
        public void saveFile(String name, String mime, String base64) {
            final String safe = name.replaceAll("[\\\\/:*?\"<>|]", "_");
            try {
                byte[] data = Base64.decode(base64, Base64.DEFAULT);
                String where;
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    ContentValues v = new ContentValues();
                    v.put(MediaStore.Downloads.DISPLAY_NAME, safe);
                    v.put(MediaStore.Downloads.MIME_TYPE, mime);
                    v.put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/Gran");
                    Uri uri = getContentResolver().insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, v);
                    if (uri == null) throw new IllegalStateException("MediaStore");
                    try (OutputStream out = getContentResolver().openOutputStream(uri)) {
                        if (out == null) throw new IllegalStateException("MediaStore");
                        out.write(data);
                    }
                    where = "Pobrane/Gran";
                } else {
                    File dir = new File(getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS), "Gran");
                    //noinspection ResultOfMethodCallIgnored
                    dir.mkdirs();
                    try (OutputStream out = new FileOutputStream(new File(dir, safe))) {
                        out.write(data);
                    }
                    where = dir.getAbsolutePath();
                }
                final String msg = "Zapisano: " + where + "/" + safe;
                runOnUiThread(() -> Toast.makeText(MainActivity.this, msg, Toast.LENGTH_LONG).show());
            } catch (Exception e) {
                runOnUiThread(() -> Toast.makeText(MainActivity.this, "Nie udało się zapisać pliku.", Toast.LENGTH_LONG).show());
            }
        }

        @JavascriptInterface
        public void installUpdate(String url, String version) {
            if (url == null || !url.startsWith("https://github.com/BlackSingularities/gran-3d/releases/download/")) return;
            downloadUpdate(url, version == null ? "nowa" : version);
        }

        @JavascriptInterface
        public void openExternal(String url) {
            if (url == null || !(url.startsWith("https://") || url.startsWith("http://"))) return;
            runOnUiThread(() -> {
                try {
                    startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
                } catch (Exception ignored) {
                    // brak przeglądarki
                }
            });
        }
    }
}
