package com.blacksingularities.gran3d;

import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.OpenableColumns;
import android.provider.Settings;
import android.view.WindowManager;
import android.widget.Toast;

import androidx.core.content.FileProvider;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.Locale;

/**
 * GRAŃ na Androida. Cały program (mapa 3D, interfejs, dane) to biblioteka natywna z Rusta
 * ({@code libgran_android.so}) uruchamiana przez {@link NativeActivity}. Ta klasa dodaje tylko to,
 * czego nie da się zrobić z kodu natywnego: okna systemu (wybór i zapis pliku), udostępnianie,
 * instalację aktualizacji i otwieranie linków. Rust wywołuje te metody przez JNI, a wyniki okien
 * odbiera co klatkę z kolejki {@link #pollEvents()}.
 */
public class GranActivity extends NativeActivity {
    private static final int PICK_TRACK = 41;
    private static final int CREATE_DOCUMENT = 42;

    private final ArrayList<String> events = new ArrayList<>();
    /** Wymuszony interfejs grafiki z intencji (`--es gran_backend vulkan|gl`) – testy w emulatorze. */
    private String backend = "";
    private String pendingSource;
    private String pendingName;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        if (getIntent() != null && getIntent().getStringExtra("gran_backend") != null) backend = getIntent().getStringExtra("gran_backend");
        handleIntent(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        handleIntent(intent);
    }

    /** Plik otwarty z innej aplikacji („Otwórz za pomocą GRANI”, „Udostępnij”). */
    private void handleIntent(Intent intent) {
        if (intent == null) return;
        // link gran://widok?… (z przeglądarki albo `am start --es gran_link …`)
        String link = intent.getStringExtra("gran_link");
        if (link == null && intent.getData() != null && "gran".equals(intent.getData().getScheme())) {
            link = intent.getData().toString();
        }
        if (link != null) {
            if (intent.getBooleanExtra("gran_capture", false)) push("capture", "1");
            push("link", link);
            return;
        }
        Uri uri = null;
        if (Intent.ACTION_VIEW.equals(intent.getAction())) {
            uri = intent.getData();
        } else if (Intent.ACTION_SEND.equals(intent.getAction())) {
            uri = intent.getParcelableExtra(Intent.EXTRA_STREAM);
        }
        if (uri != null) importUri(uri);
    }

    private synchronized void push(String kind, String payload) {
        events.add(kind + "\t" + payload.replace('\n', ' '));
    }

    /** Zdarzenia od ostatniego odczytu, wiersz po wierszu: rodzaj TAB treść. */
    public synchronized String pollEvents() {
        if (events.isEmpty()) return "";
        String out = String.join("\n", events);
        events.clear();
        return out;
    }

    // ---------------------------------------------------------------- informacje

    public String filesDir() {
        return getFilesDir().getAbsolutePath();
    }

    public String cacheDir() {
        return getCacheDir().getAbsolutePath();
    }

    public String language() {
        return Locale.getDefault().toLanguageTag();
    }

    /** Pasek stanu i pasek nawigacji (piksele): „góra dół lewo prawo”. */
    public String insets() {
        android.view.WindowInsets in = getWindow().getDecorView().getRootWindowInsets();
        if (in == null) return "0 0 0 0";
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            android.graphics.Insets i = in.getInsets(android.view.WindowInsets.Type.systemBars() | android.view.WindowInsets.Type.displayCutout());
            return i.top + " " + i.bottom + " " + i.left + " " + i.right;
        }
        return in.getSystemWindowInsetTop() + " " + in.getSystemWindowInsetBottom() + " "
                + in.getSystemWindowInsetLeft() + " " + in.getSystemWindowInsetRight();
    }

    public String backend() {
        return backend;
    }

    public String isEmulator() {
        String f = Build.FINGERPRINT + " " + Build.MODEL + " " + Build.HARDWARE + " " + Build.PRODUCT;
        boolean emu = f.contains("generic") || f.contains("emulator") || f.contains("sdk_gphone") || f.contains("ranchu") || f.contains("goldfish") || f.contains("Emulator");
        return emu ? "1" : "0";
    }

    public String deviceInfo() {
        return "Android " + Build.VERSION.RELEASE + " (API " + Build.VERSION.SDK_INT + ") · "
                + Build.MANUFACTURER + " " + Build.MODEL;
    }

    /** Miejsca na dane: pamięć wewnętrzna aplikacji, pamięć urządzenia i karty SD (bez uprawnień). */
    public String storageDirs() {
        StringBuilder sb = new StringBuilder();
        sb.append("internal\t").append(getFilesDir().getAbsolutePath());
        File[] dirs = getExternalFilesDirs(null);
        for (File d : dirs) {
            if (d == null) continue;
            boolean removable = false;
            try {
                removable = Environment.isExternalStorageRemovable(d);
            } catch (IllegalArgumentException ignored) {
            }
            if (!Environment.MEDIA_MOUNTED.equals(Environment.getExternalStorageState(d))) continue;
            sb.append('\n').append(removable ? "sd" : "primary").append('\t').append(d.getAbsolutePath());
        }
        return sb.toString();
    }

    // ---------------------------------------------------------------- działania

    public String openUrl(String url) {
        runOnUiThread(() -> {
            try {
                startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
            } catch (ActivityNotFoundException e) {
                push("msg", "Brak aplikacji do otwarcia tego adresu.");
            }
        });
        return "";
    }

    public String toast(String text) {
        runOnUiThread(() -> Toast.makeText(this, text, Toast.LENGTH_SHORT).show());
        return "";
    }

    public String moveToBack() {
        runOnUiThread(() -> moveTaskToBack(true));
        return "";
    }

    private Uri shareUri(File f) {
        return FileProvider.getUriForFile(this, getPackageName() + ".files", f);
    }

    /** Udostępnienie pliku (dziennik programu) – e-mail, komunikator, Dysk. */
    public String shareFile(String path, String mime) {
        runOnUiThread(() -> {
            try {
                Uri uri = shareUri(new File(path));
                Intent send = new Intent(Intent.ACTION_SEND);
                send.setType(mime);
                send.putExtra(Intent.EXTRA_STREAM, uri);
                send.setClipData(ClipData.newRawUri("", uri));
                send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                startActivity(Intent.createChooser(send, null));
            } catch (Exception e) {
                push("msg", "Nie udało się udostępnić pliku.");
            }
        });
        return "";
    }

    /** Okno „Otwórz” dla śladów GPX, KML, GeoJSON. */
    public String pickTrack() {
        runOnUiThread(() -> {
            Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            i.addCategory(Intent.CATEGORY_OPENABLE);
            i.setType("*/*");
            try {
                startActivityForResult(i, PICK_TRACK);
            } catch (ActivityNotFoundException e) {
                push("msg", "Brak okna wyboru plików.");
            }
        });
        return "";
    }

    /** Okno „Zapisz jako”: plik z folderu tymczasowego trafia pod adres wskazany przez użytkownika. */
    public String createDocument(String path, String name, String mime) {
        runOnUiThread(() -> {
            pendingSource = path;
            pendingName = name;
            Intent i = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            i.addCategory(Intent.CATEGORY_OPENABLE);
            i.setType(mime);
            i.putExtra(Intent.EXTRA_TITLE, name);
            try {
                startActivityForResult(i, CREATE_DOCUMENT);
            } catch (ActivityNotFoundException e) {
                push("msg", "Brak okna zapisu plików.");
            }
        });
        return "";
    }

    /** Instalacja pobranej aktualizacji oknem instalatora systemu. Pusty wynik – okno otwarte. */
    public String installApk(String path) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !getPackageManager().canRequestPackageInstalls()) {
            runOnUiThread(() -> {
                try {
                    startActivity(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + getPackageName())));
                } catch (ActivityNotFoundException ignored) {
                }
            });
            return "denied";
        }
        try {
            Uri uri = shareUri(new File(path));
            Intent i = new Intent(Intent.ACTION_VIEW);
            i.setDataAndType(uri, "application/vnd.android.package-archive");
            i.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_ACTIVITY_NEW_TASK);
            runOnUiThread(() -> startActivity(i));
            return "";
        } catch (Exception e) {
            return String.valueOf(e.getMessage());
        }
    }

    // ---------------------------------------------------------------- wyniki okien

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (result != RESULT_OK || data == null || data.getData() == null) return;
        Uri uri = data.getData();
        if (request == PICK_TRACK) {
            importUri(uri);
        } else if (request == CREATE_DOCUMENT && pendingSource != null) {
            String src = pendingSource;
            String name = pendingName;
            new Thread(() -> {
                try (InputStream in = new FileInputStream(src); OutputStream out = getContentResolver().openOutputStream(uri, "wt")) {
                    copy(in, out);
                    push("saved", name);
                } catch (Exception e) {
                    push("msg", "Nie udało się zapisać pliku.");
                }
            }).start();
        }
    }

    /** Kopia wskazanego pliku do folderu tymczasowego i zgłoszenie programowi. */
    private void importUri(Uri uri) {
        new Thread(() -> {
            String name = displayName(uri);
            File dir = new File(getCacheDir(), "import");
            //noinspection ResultOfMethodCallIgnored
            dir.mkdirs();
            File out = new File(dir, name);
            try (InputStream in = getContentResolver().openInputStream(uri); OutputStream o = new FileOutputStream(out)) {
                if (in == null) throw new IllegalStateException();
                copy(in, o);
                push("open", out.getAbsolutePath());
            } catch (Exception e) {
                push("msg", "Nie udało się wczytać pliku.");
            }
        }).start();
    }

    private String displayName(Uri uri) {
        String name = null;
        try (Cursor c = getContentResolver().query(uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst()) name = c.getString(0);
        } catch (Exception ignored) {
        }
        if (name == null || name.isEmpty()) name = uri.getLastPathSegment();
        if (name == null || name.isEmpty()) name = "slad.gpx";
        name = name.replaceAll("[\\\\/:*?\"<>|]", "_");
        if (!name.contains(".")) name += ".gpx";
        return name;
    }

    private static void copy(InputStream in, OutputStream out) throws java.io.IOException {
        byte[] buf = new byte[1 << 16];
        int n;
        while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
    }
}
