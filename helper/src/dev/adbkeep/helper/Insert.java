package dev.adbkeep.helper;

import android.content.ContentValues;
import android.net.Uri;
import android.os.Build;
import android.util.Base64;

import java.io.BufferedReader;
import java.io.FileReader;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.Set;

import org.json.JSONArray;
import org.json.JSONObject;

/**
 * Restoring rows saved by `query` (JSON lines): plain inserts, and the linked tables of
 * contacts and calendars, whose rows refer to each other by IDs that change on the target.
 * Each command prints one JSON summary line: {"inserted", "failed", "errors": [first few]}.
 */
final class Insert {
    private Insert() {}

    static final class Summary {
        int inserted;
        int failed;
        final JSONArray errors = new JSONArray();

        void fail(Throwable t) {
            failed++;
            if (errors.length() < 5) {
                Throwable c = t.getCause() != null ? t.getCause() : t;
                errors.put(String.valueOf(c));
            }
        }

        void print() throws Exception {
            JSONObject o = new JSONObject();
            o.put("inserted", inserted);
            o.put("failed", failed);
            o.put("errors", errors);
            System.out.println(o.toString());
        }
    }

    /** insert URI FILE COLUMNS: inserts every row, keeping only the comma-separated COLUMNS. */
    static void insert(String[] args) throws Exception {
        Uri uri = Uri.parse(args[0]);
        Set<String> cols = columns(args[2]);
        Summary s = new Summary();
        Providers.Holder h = Providers.acquire(uri.getAuthority());
        try {
            for (JSONObject row : read(args[1])) {
                try {
                    if (callInsert(h.provider, uri, values(row, cols, null)) == null) {
                        // e.g. content://sms ignores writers that are not the default SMS app.
                        throw new IllegalStateException("provider rejected the row");
                    }
                    s.inserted++;
                } catch (Throwable t) {
                    s.fail(t);
                }
            }
        } finally {
            h.release();
        }
        s.print();
    }

    /**
     * contacts RAW DATA RAW_COLUMNS DATA_COLUMNS: one raw contact per saved raw contact (on the
     * phone, without an account), then its data rows pointing at the new raw contact.
     */
    static void contacts(String[] args) throws Exception {
        Uri rawUri = Uri.parse("content://com.android.contacts/raw_contacts");
        Uri dataUri = Uri.parse("content://com.android.contacts/data");
        Set<String> rawCols = columns(args[2]);
        Set<String> dataCols = columns(args[3]);
        Summary s = new Summary();
        Map<Long, Long> ids = new HashMap<>();
        Providers.Holder h = Providers.acquire("com.android.contacts");
        try {
            for (JSONObject raw : read(args[0])) {
                try {
                    ContentValues v = values(raw, rawCols, null);
                    // Accounts of the source phone may not exist here: keep them on the phone.
                    v.putNull("account_type");
                    v.putNull("account_name");
                    Uri created = (Uri) callInsert(h.provider, rawUri, v);
                    ids.put(raw.getLong("_id"), Long.parseLong(created.getLastPathSegment()));
                    s.inserted++;
                } catch (Throwable t) {
                    s.fail(t);
                }
            }
            for (JSONObject data : read(args[1])) {
                Long target = ids.get(data.optLong("raw_contact_id", -1));
                if (target == null) continue;
                try {
                    ContentValues v = values(data, dataCols, null);
                    v.put("raw_contact_id", target);
                    callInsert(h.provider, dataUri, v);
                } catch (Throwable t) {
                    s.fail(t);
                }
            }
        } finally {
            h.release();
        }
        s.print();
    }

    /**
     * calendar CALENDARS EVENTS REMINDERS ATTENDEES CAL_COLS EVENT_COLS REMINDER_COLS ATTENDEE_COLS:
     * local calendars (as the sync adapter of their LOCAL account), then events, reminders and
     * attendees with the new IDs.
     */
    static void calendar(String[] args) throws Exception {
        Set<String> calCols = columns(args[4]);
        Set<String> eventCols = columns(args[5]);
        Set<String> reminderCols = columns(args[6]);
        Set<String> attendeeCols = columns(args[7]);
        Summary s = new Summary();
        Map<Long, Long> calIds = new HashMap<>();
        Map<Long, Long> eventIds = new HashMap<>();
        Providers.Holder h = Providers.acquire("com.android.calendar");
        try {
            for (JSONObject cal : read(args[0])) {
                try {
                    String account = cal.optString("account_name", "adbkeep");
                    Uri uri = Uri.parse("content://com.android.calendar/calendars").buildUpon()
                            .appendQueryParameter("caller_is_syncadapter", "true")
                            .appendQueryParameter("account_name", account)
                            .appendQueryParameter("account_type", "LOCAL")
                            .build();
                    ContentValues v = values(cal, calCols, null);
                    v.put("account_name", account);
                    v.put("account_type", "LOCAL");
                    Uri created = (Uri) callInsert(h.provider, uri, v);
                    calIds.put(cal.getLong("_id"), Long.parseLong(created.getLastPathSegment()));
                    s.inserted++;
                } catch (Throwable t) {
                    s.fail(t);
                }
            }
            Uri events = Uri.parse("content://com.android.calendar/events");
            for (JSONObject ev : read(args[1])) {
                Long cal = calIds.get(ev.optLong("calendar_id", -1));
                if (cal == null) continue;
                try {
                    ContentValues v = values(ev, eventCols, null);
                    v.put("calendar_id", cal);
                    Uri created = (Uri) callInsert(h.provider, events, v);
                    eventIds.put(ev.getLong("_id"), Long.parseLong(created.getLastPathSegment()));
                    s.inserted++;
                } catch (Throwable t) {
                    s.fail(t);
                }
            }
            linked(h, "content://com.android.calendar/reminders", args[2], reminderCols, eventIds, s);
            linked(h, "content://com.android.calendar/attendees", args[3], attendeeCols, eventIds, s);
        } finally {
            h.release();
        }
        s.print();
    }

    private static void linked(Providers.Holder h, String uriText, String file, Set<String> cols, Map<Long, Long> eventIds, Summary s)
            throws Exception {
        Uri uri = Uri.parse(uriText);
        for (JSONObject row : read(file)) {
            Long event = eventIds.get(row.optLong("event_id", -1));
            if (event == null) continue;
            try {
                ContentValues v = values(row, cols, null);
                v.put("event_id", event);
                callInsert(h.provider, uri, v);
            } catch (Throwable t) {
                s.fail(t);
            }
        }
    }

    static Set<String> columns(String list) {
        Set<String> set = new HashSet<>();
        for (String c : list.split(",")) if (!c.isEmpty()) set.add(c);
        return set;
    }

    static List<JSONObject> read(String path) throws Exception {
        List<JSONObject> rows = new ArrayList<>();
        try (BufferedReader r = new BufferedReader(new FileReader(path))) {
            String line;
            while ((line = r.readLine()) != null) {
                if (line.startsWith("{")) rows.add(new JSONObject(line));
            }
        } catch (java.io.FileNotFoundException e) {
            // A table with no rows may have no file.
        }
        return rows;
    }

    /** JSON row → ContentValues with the allowed columns; nulls are left out. */
    static ContentValues values(JSONObject row, Set<String> allowed, ContentValues into) throws Exception {
        ContentValues v = into != null ? into : new ContentValues();
        Iterator<String> keys = row.keys();
        while (keys.hasNext()) {
            String k = keys.next();
            if (!allowed.contains(k) || row.isNull(k)) continue;
            Object o = row.get(k);
            if (o instanceof JSONObject && ((JSONObject) o).has("base64")) {
                v.put(k, Base64.decode(((JSONObject) o).getString("base64"), Base64.DEFAULT));
            } else if (o instanceof Integer || o instanceof Long) {
                v.put(k, ((Number) o).longValue());
            } else if (o instanceof Number) {
                v.put(k, ((Number) o).doubleValue());
            } else if (o instanceof Boolean) {
                v.put(k, (Boolean) o);
            } else {
                v.put(k, String.valueOf(o));
            }
        }
        return v;
    }

    /** IContentProvider.insert, whose signature changed in Android 12. */
    static Object callInsert(Object provider, Uri uri, ContentValues values) throws Exception {
        String pkg = Reflect.callingPackage();
        if (Build.VERSION.SDK_INT >= 31) {
            Method m = Reflect.method(provider.getClass(), "insert", 4, android.content.AttributionSource.class);
            return m.invoke(provider, Providers.attribution(pkg), uri, values, null);
        }
        Method m = Reflect.method(provider.getClass(), "insert", 5, String.class);
        return m.invoke(provider, pkg, null, uri, values, null);
    }
}
