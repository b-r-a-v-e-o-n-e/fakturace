package cz.fakturace.app;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.ContentValues;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Color;
import android.graphics.pdf.PdfRenderer;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.os.ParcelFileDescriptor;
import android.provider.MediaStore;
import android.util.Base64;
import android.view.Gravity;
import android.webkit.CookieManager;
import android.webkit.JavascriptInterface;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.TextView;

import androidx.core.content.FileProvider;

import org.json.JSONArray;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.OutputStream;

public class MainActivity extends Activity {
    private static final int REQUEST_FILE = 1;
    private static final int MAX_PREVIEW_PAGES = 8;
    private static final int PREVIEW_WIDTH = 1100;

    private WebView web;
    private String origin = "";
    private ValueCallback<Uri[]> fileCallback;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        int port = NativeServer.ensureStarted(getApplicationContext());
        if (port <= 0) {
            TextView msg = new TextView(this);
            msg.setText("Aplikaci se nepodařilo spustit. Zavřete ji a otevřete znovu.");
            msg.setGravity(Gravity.CENTER);
            msg.setPadding(48, 48, 48, 48);
            setContentView(msg);
            return;
        }
        origin = "http://127.0.0.1:" + port;

        // Server pustí jen požadavky s tímto tajným cookie – ostatní aplikace v telefonu se k datům nedostanou.
        CookieManager cookies = CookieManager.getInstance();
        cookies.setAcceptCookie(true);
        cookies.setCookie(origin, "fakturace_token=" + NativeServer.token() + "; Path=/");
        cookies.flush();

        web = new WebView(this);
        setContentView(web);

        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setAllowFileAccess(false);
        s.setAllowContentAccess(false);
        s.setSupportZoom(true);
        s.setBuiltInZoomControls(true);
        s.setDisplayZoomControls(false);
        s.setTextZoom(100);

        web.addJavascriptInterface(new Bridge(), "Android");

        web.setWebViewClient(new WebViewClient() {
            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                Uri url = request.getUrl();
                if (url.toString().startsWith(origin + "/")) {
                    return false;
                }
                try {
                    startActivity(new Intent(Intent.ACTION_VIEW, url));
                } catch (Exception ignored) {
                    // žádná aplikace pro odkaz
                }
                return true;
            }
        });

        web.setWebChromeClient(new WebChromeClient() {
            @Override
            public boolean onShowFileChooser(WebView view, ValueCallback<Uri[]> callback, FileChooserParams params) {
                if (fileCallback != null) {
                    fileCallback.onReceiveValue(null);
                }
                fileCallback = callback;
                Intent pick = new Intent(Intent.ACTION_GET_CONTENT);
                pick.addCategory(Intent.CATEGORY_OPENABLE);
                boolean images = false;
                String[] accept = params.getAcceptTypes();
                if (accept != null) {
                    for (String a : accept) {
                        if (a != null && a.startsWith("image/")) {
                            images = true;
                        }
                    }
                }
                pick.setType(images ? "image/*" : "*/*");
                try {
                    startActivityForResult(Intent.createChooser(pick, "Vyberte soubor"), REQUEST_FILE);
                } catch (ActivityNotFoundException e) {
                    fileCallback = null;
                    callback.onReceiveValue(null);
                }
                return true;
            }
        });

        if (savedInstanceState != null) {
            web.restoreState(savedInstanceState);
        } else {
            web.loadUrl(origin + "/");
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQUEST_FILE && fileCallback != null) {
            fileCallback.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(resultCode, data));
            fileCallback = null;
        }
    }

    @Override
    protected void onSaveInstanceState(Bundle outState) {
        super.onSaveInstanceState(outState);
        if (web != null) {
            web.saveState(outState);
        }
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        if (web != null && web.canGoBack()) {
            web.goBack();
        } else {
            super.onBackPressed();
        }
    }

    private static String safeName(String name) {
        String n = name == null ? "" : name.replaceAll("[^A-Za-z0-9._-]", "_");
        return n.isEmpty() ? "soubor" : n;
    }

    private File writeShared(String base64, String name) throws Exception {
        File dir = new File(getCacheDir(), "shared");
        if (!dir.exists() && !dir.mkdirs()) {
            throw new IllegalStateException("cache");
        }
        File[] old = dir.listFiles();
        if (old != null) {
            for (File f : old) {
                f.delete();
            }
        }
        File out = new File(dir, safeName(name));
        try (FileOutputStream os = new FileOutputStream(out)) {
            os.write(Base64.decode(base64, Base64.DEFAULT));
        }
        return out;
    }

    private void share(File file, String mime, String title) {
        Uri uri = FileProvider.getUriForFile(this, getPackageName() + ".files", file);
        Intent send = new Intent(Intent.ACTION_SEND);
        send.setType(mime);
        send.putExtra(Intent.EXTRA_STREAM, uri);
        send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        startActivity(Intent.createChooser(send, title));
    }

    /** Funkce volané z JavaScriptu jako window.Android.*. Vracejí text pro krátké oznámení, nebo prázdný řetězec. */
    private final class Bridge {
        /** Vykreslí stránky PDF na obrázky (WebView neumí PDF zobrazit). Vrací JSON pole data-URL. */
        @JavascriptInterface
        public String renderPdf(String base64) {
            JSONArray pages = new JSONArray();
            File tmp = null;
            try {
                tmp = File.createTempFile("nahled", ".pdf", getCacheDir());
                try (FileOutputStream os = new FileOutputStream(tmp)) {
                    os.write(Base64.decode(base64, Base64.DEFAULT));
                }
                try (ParcelFileDescriptor fd = ParcelFileDescriptor.open(tmp, ParcelFileDescriptor.MODE_READ_ONLY);
                     PdfRenderer renderer = new PdfRenderer(fd)) {
                    int count = Math.min(renderer.getPageCount(), MAX_PREVIEW_PAGES);
                    for (int i = 0; i < count; i++) {
                        PdfRenderer.Page page = renderer.openPage(i);
                        int height = Math.round(PREVIEW_WIDTH * (float) page.getHeight() / page.getWidth());
                        Bitmap bmp = Bitmap.createBitmap(PREVIEW_WIDTH, height, Bitmap.Config.ARGB_8888);
                        bmp.eraseColor(Color.WHITE);
                        page.render(bmp, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY);
                        page.close();
                        ByteArrayOutputStream jpeg = new ByteArrayOutputStream();
                        bmp.compress(Bitmap.CompressFormat.JPEG, 88, jpeg);
                        bmp.recycle();
                        pages.put("data:image/jpeg;base64," + Base64.encodeToString(jpeg.toByteArray(), Base64.NO_WRAP));
                    }
                }
            } catch (Exception e) {
                // náhled se nezobrazí, zbytek aplikace funguje dál
            } finally {
                if (tmp != null) {
                    tmp.delete();
                }
            }
            return pages.toString();
        }

        /** Otevře soubor (PDF) v aplikaci, kterou si uživatel vybere; odtud jde i sdílet a tisknout. */
        @JavascriptInterface
        public String openFile(String base64, String name, String mime) {
            try {
                final File file = writeShared(base64, name);
                final String type = mime;
                runOnUiThread(() -> {
                    try {
                        Uri uri = FileProvider.getUriForFile(MainActivity.this, getPackageName() + ".files", file);
                        Intent view = new Intent(Intent.ACTION_VIEW);
                        view.setDataAndType(uri, type);
                        view.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                        startActivity(view);
                    } catch (ActivityNotFoundException e) {
                        share(file, type, "Otevřít nebo sdílet");
                    }
                });
                return "";
            } catch (Exception e) {
                return "Soubor se nepodařilo otevřít.";
            }
        }

        /** Uloží soubor do složky Stažené (Android 10+), na starších systémech nabídne sdílení. */
        @JavascriptInterface
        public String saveFile(String base64, String name, String mime) {
            String fileName = safeName(name);
            try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    // Neznámé typy (např. .isdoc) ukládáme jako obecný soubor, aby systém nezměnil příponu.
                    boolean known = "application/pdf".equals(mime) || "text/csv".equals(mime) || "application/json".equals(mime);
                    ContentValues values = new ContentValues();
                    values.put(MediaStore.Downloads.DISPLAY_NAME, fileName);
                    values.put(MediaStore.Downloads.MIME_TYPE, known ? mime : "application/octet-stream");
                    values.put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS);
                    Uri target = getContentResolver().insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values);
                    if (target == null) {
                        return "Soubor se nepodařilo uložit.";
                    }
                    try (OutputStream os = getContentResolver().openOutputStream(target)) {
                        if (os == null) {
                            return "Soubor se nepodařilo uložit.";
                        }
                        os.write(Base64.decode(base64, Base64.DEFAULT));
                    }
                    return "Uloženo do složky Stažené: " + fileName;
                }
                final File file = writeShared(base64, fileName);
                runOnUiThread(() -> share(file, "application/octet-stream", "Uložit nebo sdílet"));
                return "";
            } catch (Exception e) {
                return "Soubor se nepodařilo uložit.";
            }
        }
    }
}
