package dev.adbkeep.helper;

import android.content.pm.ApplicationInfo;
import android.content.res.AssetManager;
import android.content.res.Configuration;
import android.content.res.Resources;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.drawable.Drawable;
import android.util.Base64;
import android.util.DisplayMetrics;

import java.io.ByteArrayOutputStream;
import java.lang.reflect.Method;

import org.json.JSONObject;

/** Name and icon of each package, for the app lists (ui-spec §3.1 "Icono y nombre"). */
final class Labels {
    private static final int ICON_PX = 96;

    private Labels() {}

    static void run(String[] packages) throws Exception {
        Object pm = Reflect.service("package", "android.content.pm.IPackageManager$Stub");
        // getApplicationInfo(String, int|long flags, int userId) depending on the version.
        Method get = Reflect.method(pm.getClass(), "getApplicationInfo", 3, String.class);
        boolean longFlags = get.getParameterTypes()[1] == long.class;
        for (String pkg : packages) {
            JSONObject out = new JSONObject();
            out.put("package", pkg);
            try {
                ApplicationInfo ai = (ApplicationInfo) get.invoke(pm, pkg, longFlags ? (Object) 0L : (Object) 0, 0);
                if (ai == null) throw new IllegalStateException("not installed");
                Resources res = resources(ai);
                out.put("label", label(ai, res));
                String icon = icon(ai, res);
                if (icon != null) out.put("icon", icon);
            } catch (Throwable t) {
                Throwable cause = t.getCause() != null ? t.getCause() : t;
                out.put("error", String.valueOf(cause));
            }
            System.out.println(out.toString());
        }
    }

    private static Resources resources(ApplicationInfo ai) throws Exception {
        AssetManager assets = AssetManager.class.getDeclaredConstructor().newInstance();
        Method add = AssetManager.class.getMethod("addAssetPath", String.class);
        add.invoke(assets, ai.sourceDir);
        if (ai.splitSourceDirs != null) {
            for (String split : ai.splitSourceDirs) add.invoke(assets, split);
        }
        Resources system = Resources.getSystem();
        DisplayMetrics metrics = system.getDisplayMetrics();
        Configuration config = new Configuration(system.getConfiguration());
        return new Resources(assets, metrics, config);
    }

    private static String label(ApplicationInfo ai, Resources res) {
        if (ai.nonLocalizedLabel != null) return ai.nonLocalizedLabel.toString();
        if (ai.labelRes != 0) {
            try {
                return res.getText(ai.labelRes).toString();
            } catch (Resources.NotFoundException ignored) {
                // Falls through to the package name.
            }
        }
        return ai.packageName;
    }

    private static String icon(ApplicationInfo ai, Resources res) {
        if (ai.icon == 0) return null;
        try {
            Drawable d = res.getDrawable(ai.icon, null);
            Bitmap bmp = Bitmap.createBitmap(ICON_PX, ICON_PX, Bitmap.Config.ARGB_8888);
            Canvas canvas = new Canvas(bmp);
            d.setBounds(0, 0, ICON_PX, ICON_PX);
            d.draw(canvas);
            ByteArrayOutputStream png = new ByteArrayOutputStream();
            bmp.compress(Bitmap.CompressFormat.PNG, 100, png);
            return Base64.encodeToString(png.toByteArray(), Base64.NO_WRAP);
        } catch (Throwable t) {
            return null;
        }
    }
}
