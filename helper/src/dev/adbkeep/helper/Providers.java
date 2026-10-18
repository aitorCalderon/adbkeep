package dev.adbkeep.helper;

import android.content.ContentResolver;
import android.content.res.AssetFileDescriptor;
import android.database.Cursor;
import android.net.Uri;
import android.os.Binder;
import android.os.Build;
import android.os.Bundle;
import android.os.IBinder;
import android.util.Base64;

import java.io.InputStream;
import java.io.OutputStream;
import java.lang.reflect.Method;

import org.json.JSONObject;

/**
 * Content providers through the activity manager, like the `content` shell command does
 * (frameworks/base/cmds/content), without needing an Android Context.
 */
final class Providers {
    private static final int USER = 0;

    private Providers() {}

    /** query URI [--projection a,b] [--where SQL] [--sort SQL]: one JSON object per row. */
    static void query(String[] args) throws Exception {
        query(Uri.parse(args[0]), args, 1, null);
    }

    static void queryEach(String[] args) throws Exception {
        String template = args[0];
        for (String id : args[1].split(",")) {
            if (id.isEmpty()) continue;
            query(Uri.parse(template.replace("{id}", id)), args, 2, id);
        }
    }

    static void batch(String[] args) throws Exception {
        for (int i = 0, q = 0; i + 2 < args.length; i += 3, q++) {
            Uri uri = Uri.parse(args[i]);
            String projection = args[i + 1];
            String where = args[i + 2];
            Bundle queryArgs = new Bundle();
            if (!"-".equals(where)) queryArgs.putString(ContentResolver.QUERY_ARG_SQL_SELECTION, where);
            boolean count = "#count".equals(projection);
            String[] cols = count ? new String[] {"_id"} : "-".equals(projection) ? null : projection.split(",");
            try {
                Holder h = acquire(uri.getAuthority());
                try {
                    Cursor c = (Cursor) callQuery(h.provider, uri, cols, queryArgs);
                    if (count) {
                        JSONObject row = new JSONObject();
                        row.put("_q", q);
                        row.put("count", c == null ? 0 : c.getCount());
                        System.out.println(row.toString());
                    } else if (c != null) {
                        printRows(c, null, q);
                    }
                    if (c != null) c.close();
                } finally {
                    h.release();
                }
            } catch (Throwable t) {
                Throwable cause = t.getCause() != null ? t.getCause() : t;
                JSONObject row = new JSONObject();
                row.put("_q", q);
                row.put("error", String.valueOf(cause));
                System.out.println(row.toString());
            }
        }
    }

    private static void query(Uri uri, String[] args, int firstOption, String idOf) throws Exception {
        String[] projection = null;
        Bundle queryArgs = new Bundle();
        for (int i = firstOption; i + 1 < args.length; i += 2) {
            switch (args[i]) {
                case "--projection":
                    projection = args[i + 1].split(",");
                    break;
                case "--where":
                    queryArgs.putString(ContentResolver.QUERY_ARG_SQL_SELECTION, args[i + 1]);
                    break;
                case "--sort":
                    queryArgs.putString(ContentResolver.QUERY_ARG_SQL_SORT_ORDER, args[i + 1]);
                    break;
                default:
                    throw new IllegalArgumentException("unknown option " + args[i]);
            }
        }
        Holder h = acquire(uri.getAuthority());
        try {
            Cursor c = (Cursor) callQuery(h.provider, uri, projection, queryArgs);
            if (c == null) return;
            try {
                printRows(c, idOf, -1);
            } finally {
                c.close();
            }
        } finally {
            h.release();
        }
    }

    /** One JSON object per row, with typed values; `idOf`/`q` tag rows of query-each/batch. */
    private static void printRows(Cursor c, String idOf, int q) throws Exception {
        {
            {
                String[] cols = c.getColumnNames();
                while (c.moveToNext()) {
                    JSONObject row = new JSONObject();
                    if (idOf != null) row.put("_id_of", idOf);
                    if (q >= 0) row.put("_q", q);
                    for (int i = 0; i < cols.length; i++) {
                        switch (c.getType(i)) {
                            case Cursor.FIELD_TYPE_NULL:
                                row.put(cols[i], JSONObject.NULL);
                                break;
                            case Cursor.FIELD_TYPE_INTEGER:
                                row.put(cols[i], c.getLong(i));
                                break;
                            case Cursor.FIELD_TYPE_FLOAT:
                                row.put(cols[i], c.getDouble(i));
                                break;
                            case Cursor.FIELD_TYPE_BLOB:
                                JSONObject blob = new JSONObject();
                                blob.put("base64", Base64.encodeToString(c.getBlob(i), Base64.NO_WRAP));
                                row.put(cols[i], blob);
                                break;
                            default:
                                row.put(cols[i], c.getString(i));
                        }
                    }
                    System.out.println(row.toString());
                }
            }
        }
    }

    /** Copies what openAssetFile(URI, "r") returns to `out`. */
    static void asset(String uriText, OutputStream out) throws Exception {
        Uri uri = Uri.parse(uriText);
        Holder h = acquire(uri.getAuthority());
        try {
            AssetFileDescriptor afd = (AssetFileDescriptor) callOpenAsset(h.provider, uri);
            if (afd == null) throw new IllegalStateException("no data for " + uri);
            try (InputStream in = afd.createInputStream()) {
                byte[] buf = new byte[64 * 1024];
                int n;
                while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
            }
            out.flush();
        } finally {
            h.release();
        }
    }

    private static Object callQuery(Object provider, Uri uri, String[] projection, Bundle args) throws Exception {
        String pkg = Reflect.callingPackage();
        if (Build.VERSION.SDK_INT >= 31) {
            Method m = Reflect.method(provider.getClass(), "query", 5, android.content.AttributionSource.class);
            return m.invoke(provider, attribution(pkg), uri, projection, args, null);
        }
        Method m = Reflect.method(provider.getClass(), "query", 6, String.class);
        return m.invoke(provider, pkg, null, uri, projection, args, null);
    }

    private static Object callOpenAsset(Object provider, Uri uri) throws Exception {
        String pkg = Reflect.callingPackage();
        if (Build.VERSION.SDK_INT >= 31) {
            Method m = Reflect.method(provider.getClass(), "openAssetFile", 4, android.content.AttributionSource.class);
            return m.invoke(provider, attribution(pkg), uri, "r", null);
        }
        Method m = Reflect.method(provider.getClass(), "openAssetFile", 5, String.class);
        return m.invoke(provider, pkg, null, uri, "r", null);
    }

    static Object attribution(String pkg) {
        return new android.content.AttributionSource.Builder(android.os.Process.myUid()).setPackageName(pkg).build();
    }

    /** A provider reference from IActivityManager.getContentProviderExternal; release it when done. */
    static final class Holder {
        final Object am;
        final String authority;
        final IBinder token;
        final Object provider;

        Holder(Object am, String authority, IBinder token, Object provider) {
            this.am = am;
            this.authority = authority;
            this.token = token;
            this.provider = provider;
        }

        void release() {
            try {
                Reflect.method(am.getClass(), "removeContentProviderExternalAsUser", 3, String.class).invoke(am, authority, token, USER);
            } catch (Exception ignored) {
                // Released anyway when the process exits.
            }
        }
    }

    static Holder acquire(String authority) throws Exception {
        Object am = Class.forName("android.app.ActivityManager").getMethod("getService").invoke(null);
        IBinder token = new Binder();
        Method get = Reflect.method(am.getClass(), "getContentProviderExternal", 4, String.class);
        Object holder = get.invoke(am, authority, USER, token, "*adbkeep*");
        if (holder == null) throw new IllegalStateException("no provider " + authority);
        Object provider = holder.getClass().getField("provider").get(holder);
        return new Holder(am, authority, token, provider);
    }
}
