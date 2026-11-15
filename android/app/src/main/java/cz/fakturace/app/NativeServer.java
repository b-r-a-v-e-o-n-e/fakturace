package cz.fakturace.app;

import android.content.Context;

import java.io.File;
import java.security.SecureRandom;

/** Spouští server napsaný v Rustu (knihovna libfakturace.so) uvnitř procesu aplikace. */
final class NativeServer {
    /** Pevný port drží stejnou adresu stránky mezi spuštěními; je-li obsazený, zvolí se jiný. */
    private static final int PREFERRED_PORT = 47610;

    private static int port = 0;
    private static String token = "";

    static {
        System.loadLibrary("fakturace");
    }

    private NativeServer() {
    }

    private static native int start(String dataDir, int port, String token);

    /** Spustí server jen jednou za život procesu. Vrací port, nebo 0 při chybě. */
    static synchronized int ensureStarted(Context context) {
        if (port > 0) {
            return port;
        }
        byte[] raw = new byte[24];
        new SecureRandom().nextBytes(raw);
        StringBuilder sb = new StringBuilder();
        for (byte b : raw) {
            sb.append(String.format("%02x", b & 0xff));
        }
        token = sb.toString();
        File dir = new File(context.getFilesDir(), "data");
        port = start(dir.getAbsolutePath(), PREFERRED_PORT, token);
        return port;
    }

    static synchronized String token() {
        return token;
    }
}
