package dev.adbkeep.helper;

import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStream;
import java.util.Arrays;

/**
 * Runs on the phone with app_process, as root, launched by adbkeep:
 *
 * <pre>CLASSPATH=/data/local/tmp/adbkeep/helper.dex app_process /system/bin dev.adbkeep.helper.Main CMD ARGS</pre>
 *
 * Text results are JSON, one object per line, so values with commas or line breaks stay
 * unambiguous (unlike `content query`). Binary results go to stdout as they are.
 * Exit code 0 on success; errors go to stderr.
 */
public final class Main {
    /** Bumped when the command line or the output changes; adbkeep checks it. */
    static final int PROTOCOL = 4;

    public static void main(String[] args) {
        int code = 0;
        try {
            run(args);
        } catch (Throwable t) {
            System.err.println("error: " + t);
            t.printStackTrace();
            code = 1;
        }
        System.out.flush();
        System.exit(code);
    }

    static void run(String[] args) throws Exception {
        if (args.length == 0) {
            throw new IllegalArgumentException("usage: version | labels PKG... | query URI [--projection a,b] [--where SQL] [--sort SQL] | query-each TEMPLATE IDS [options] | asset URI");
        }
        String[] rest = Arrays.copyOfRange(args, 1, args.length);
        switch (args[0]) {
            case "version":
                System.out.println("{\"protocol\":" + PROTOCOL + ",\"sdk\":" + android.os.Build.VERSION.SDK_INT + "}");
                break;
            case "labels":
                Labels.run(rest);
                break;
            case "query":
                Providers.query(rest);
                break;
            case "query-each":
                // query-each TEMPLATE IDS [options]: TEMPLATE with {id} replaced by each of the
                // comma-separated IDS; rows carry "_id_of" (e.g. the addresses of every MMS).
                Providers.queryEach(rest);
                break;
            case "batch":
                // batch (URI PROJECTION WHERE)...: several queries in one process start (~0.5 s
                // each otherwise). PROJECTION "#count" returns only the row count; "-" means none.
                // Rows carry "_q", the index of their query; a failing query yields {"_q", "error"}.
                Providers.batch(rest);
                break;
            case "insert":
                Insert.insert(rest);
                break;
            case "insert-contacts":
                Insert.contacts(rest);
                break;
            case "insert-calendar":
                Insert.calendar(rest);
                break;
            case "asset":
                // vCards (contacts as_multi_vcard) and MMS parts: openAssetFile, which `content read` lacks.
                Providers.asset(rest[0], stdout());
                break;
            default:
                throw new IllegalArgumentException("unknown command " + args[0]);
        }
    }

    static OutputStream stdout() {
        return new FileOutputStream(FileDescriptor.out);
    }
}
