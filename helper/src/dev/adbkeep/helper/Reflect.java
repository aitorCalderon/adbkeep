package dev.adbkeep.helper;

import java.lang.reflect.Method;

/** Hidden-API access by name, tolerant to signature changes between Android versions. */
final class Reflect {
    private Reflect() {}

    /** The first public or declared method called `name` taking `count` parameters, whose first one is `first` (any if null). */
    static Method method(Class<?> cls, String name, int count, Class<?> first) {
        for (Class<?> c = cls; c != null; c = c.getSuperclass()) {
            for (Method m : c.getDeclaredMethods()) {
                Class<?>[] p = m.getParameterTypes();
                if (m.getName().equals(name) && p.length == count && (first == null || p.length > 0 && p[0] == first)) {
                    m.setAccessible(true);
                    return m;
                }
            }
        }
        for (Class<?> i : cls.getInterfaces()) {
            Method m = methodOrNull(i, name, count, first);
            if (m != null) return m;
        }
        return null;
    }

    private static Method methodOrNull(Class<?> cls, String name, int count, Class<?> first) {
        for (Method m : cls.getMethods()) {
            Class<?>[] p = m.getParameterTypes();
            if (m.getName().equals(name) && p.length == count && (first == null || p.length > 0 && p[0] == first)) {
                return m;
            }
        }
        return null;
    }

    static Object service(String name, String stubClass) throws Exception {
        Class<?> sm = Class.forName("android.os.ServiceManager");
        Object binder = sm.getMethod("getService", String.class).invoke(null, name);
        Class<?> stub = Class.forName(stubClass);
        return stub.getMethod("asInterface", android.os.IBinder.class).invoke(null, binder);
    }

    /** "root" for uid 0 and "com.android.shell" for 2000, as the `content` command does. */
    static String callingPackage() {
        int uid = android.os.Process.myUid();
        if (uid == 0) return "root";
        if (uid == 2000) return "com.android.shell";
        return null;
    }
}
