package com.blacksingularities.gran3d;

import android.content.ContentValues;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.MediaStore;
import android.util.Base64;
import android.webkit.JavascriptInterface;
import android.webkit.WebView;
import android.widget.Toast;

import com.getcapacitor.BridgeActivity;

import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.io.OutputStream;
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
