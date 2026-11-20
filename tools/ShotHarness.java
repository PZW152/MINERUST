import java.io.File;
import java.lang.reflect.Field;
import java.util.Arrays;
import java.util.List;
import java.util.concurrent.Callable;

import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.RenderGlobal;
import net.minecraft.entity.player.EntityPlayerMP;
import net.minecraft.server.integrated.IntegratedServer;
import net.minecraft.util.MouseHelper;
import net.minecraft.util.ScreenShotHelper;
import net.minecraft.world.WorldServer;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldSettings$GameType; // the remapped jar keeps nested classes as top-level $ names
import net.minecraft.world.WorldType;

/**
 * Runs the real 1.7.10 client and takes one screenshot of a scene, so that
 * tools/shotcompare.py can put it next to MINERUST's screenshot of the same
 * scene (same seed, time, position, view direction, window size and
 * options.txt).
 *
 * Args: out.png seed time x y z yaw pitch settleSeconds [--hide-gui] [--debug] [--setblock "x y z id meta;..."]
 *       -- <net.minecraft.client.main.Main args>
 *
 * The scene is driven from a helper thread through Minecraft.addScheduledTask,
 * so everything that touches the game runs on the client thread, as a player's
 * own actions would: create a creative world, set the time and stop the
 * daylight cycle (MINERUST --freeze-time), place the flying player
 * (MINERUST --pos/--look with --creative), wait until RenderGlobal has no
 * render chunks left to build, wait settleSeconds more, then
 * ScreenShotHelper.saveScreenshot (the F2 path) and quit.
 */
public class ShotHarness {
    public static void main(String[] a) throws Exception {
        int sep = Arrays.asList(a).indexOf("--");
        final File out = new File(a[0]).getAbsoluteFile();
        final long seed = Long.parseLong(a[1]);
        final long time = Long.parseLong(a[2]);
        final double x = Double.parseDouble(a[3]), y = Double.parseDouble(a[4]), z = Double.parseDouble(a[5]);
        final float yaw = Float.parseFloat(a[6]), pitch = Float.parseFloat(a[7]);
        final double settle = Double.parseDouble(a[8]);
        final boolean hideGui = Arrays.asList(a).subList(9, sep).contains("--hide-gui");
        final boolean debug = Arrays.asList(a).subList(9, sep).contains("--debug");
        int sb = Arrays.asList(a).subList(9, sep).indexOf("--setblock");
        final String setblock = sb < 0 ? "" : a[9 + sb + 1];
        String[] mcArgs = Arrays.copyOfRange(a, sep + 1, a.length);

        Thread driver = new Thread(new Runnable() {
            public void run() {
                try {
                    drive(out, seed, time, x, y, z, yaw, pitch, settle, hideGui, debug, setblock);
                } catch (Throwable t) {
                    t.printStackTrace();
                    System.exit(2);
                }
            }
        }, "ShotHarness");
        driver.setDaemon(true);
        driver.start();
        hostOffscreen(mcArgs);
        net.minecraft.client.main.Main.main(mcArgs);
    }

    /**
     * The tool runs while someone is using the computer. LWJGL's own window
     * would come to the front and take the keyboard focus, so the game draws
     * into a canvas inside an undecorated frame that is placed off-screen and
     * may never be focused (Display.setParent before Minecraft calls
     * Display.create). The game renders into its framebuffer object and the
     * screenshot reads that texture, so being off-screen does not matter.
     */
    private static void hostOffscreen(String[] mcArgs) throws Exception {
        int w = 854, h = 480;
        for (int i = 0; i + 1 < mcArgs.length; i++) {
            if (mcArgs[i].equals("--width")) w = Integer.parseInt(mcArgs[i + 1]);
            if (mcArgs[i].equals("--height")) h = Integer.parseInt(mcArgs[i + 1]);
        }
        final java.awt.Canvas canvas = new java.awt.Canvas();
        canvas.setPreferredSize(new java.awt.Dimension(w, h));
        canvas.setFocusable(false);
        final java.awt.Frame frame = new java.awt.Frame("vanilla 1.7.10 (screenshot tool)");
        frame.setUndecorated(true);
        frame.setType(java.awt.Window.Type.UTILITY); // no taskbar button
        frame.setFocusableWindowState(false);
        frame.setAutoRequestFocus(false);
        frame.add(canvas);
        frame.pack();
        frame.setLocation(-4 * w, -4 * h);
        frame.setVisible(true);
        org.lwjgl.opengl.Display.setParent(canvas);
    }

    private static Minecraft mc;

    private static <T> T onClient(Callable<T> task) throws Exception {
        return (T) mc.addScheduledTask(task).get();
    }

    private static void sleep(long ms) {
        try { Thread.sleep(ms); } catch (InterruptedException e) { throw new RuntimeException(e); }
    }

    static void drive(final File out, final long seed, final long time, final double x, final double y, final double z,
                      final float yaw, final float pitch, double settle, final boolean hideGui, final boolean debug,
                      final String setblock) throws Exception {
        // Title screen up: resources are loaded and the game loop runs.
        while ((mc = Minecraft.getMinecraft()) == null || mc.currentScreen == null) sleep(100);
        // Never take the user's mouse: entering the world calls setIngameFocus,
        // which grabs the cursor, and mouse movement would turn the camera.
        onClient(new Callable<Object>() {
            public Object call() {
                mc.mouseHelper = new MouseHelper() {
                    public void grabMouseCursor() {}
                    public void ungrabMouseCursor() {}
                    public void mouseXYChange() { deltaX = 0; deltaY = 0; }
                };
                return null;
            }
        });
        sleep(500);
        onClient(new Callable<Object>() {
            public Object call() {
                mc.launchIntegratedServer("shot", "shot",
                        new WorldSettings(seed, WorldSettings$GameType.CREATIVE, true, false, WorldType.DEFAULT));
                return null;
            }
        });
        // currentScreen may already be the pause menu if the window lost focus;
        // the scene setup below closes it.
        while (mc.thePlayer == null || mc.theWorld == null) sleep(100);
        while (mc.getIntegratedServer() == null
                || mc.getIntegratedServer().getConfigurationManager().playerEntityList.isEmpty()) sleep(100);

        onClient(new Callable<Object>() {
            public Object call() {
                IntegratedServer server = mc.getIntegratedServer();
                for (WorldServer w : server.worldServers) {
                    w.getGameRules().setOrCreateGameRule("doDaylightCycle", "false");
                    w.setWorldTime(time);
                }
                mc.theWorld.setWorldTime(time);
                // /setblock in the overworld, in order (supports before what hangs on them)
                for (String cmd : setblock.split(";")) {
                    String[] t = cmd.trim().split("\\s+");
                    if (t.length < 4) continue;
                    server.worldServers[0].setBlock(Integer.parseInt(t[0]), Integer.parseInt(t[1]), Integer.parseInt(t[2]),
                            net.minecraft.block.Block.getBlockById(Integer.parseInt(t[3])),
                            t.length > 4 ? Integer.parseInt(t[4]) : 0, 3);
                }
                mc.gameSettings.hideGUI = hideGui;
                mc.gameSettings.showDebugInfo = debug; // F3, like MINERUST --debug
                // A background window must not pause (options.txt also sets pauseOnLostFocus:false).
                mc.gameSettings.pauseOnLostFocus = false;
                mc.displayGuiScreen(null);
                return null;
            }
        });

        // Wait until every render chunk in view is built (RenderGlobal.worldRenderersToUpdate
        // stays empty), giving up after a minute like MINERUST's screenshot mode.
        final Field pending = RenderGlobal.class.getDeclaredField("worldRenderersToUpdate");
        pending.setAccessible(true);
        // Keeps the flying player at the scene's spot. A position update from the
        // server that was already in flight can move the client player after the
        // first placement, so this runs on every check and reports whether the
        // player had to be put back.
        final Callable<Boolean> place = new Callable<Boolean>() {
            public Boolean call() {
                // the client player's posY is eye height (yOffset); the feet are boundingBox.minY
                boolean there = Math.abs(mc.thePlayer.posX - x) < 1e-3 && Math.abs(mc.thePlayer.boundingBox.minY - y) < 1e-3
                        && Math.abs(mc.thePlayer.posZ - z) < 1e-3 && mc.thePlayer.rotationYaw == yaw
                        && mc.thePlayer.rotationPitch == pitch && mc.thePlayer.capabilities.isFlying;
                if (there) return false;
                EntityPlayerMP p = (EntityPlayerMP) mc.getIntegratedServer().getConfigurationManager().playerEntityList.get(0);
                p.capabilities.isFlying = true;
                p.sendPlayerAbilities();
                p.playerNetServerHandler.setPlayerLocation(x, y, z, yaw, pitch);
                mc.thePlayer.capabilities.isFlying = true;
                mc.thePlayer.setLocationAndAngles(x, y, z, yaw, pitch);
                mc.thePlayer.motionX = mc.thePlayer.motionY = mc.thePlayer.motionZ = 0;
                return true;
            }
        };
        long start = System.currentTimeMillis();
        int quiet = 0;
        while (quiet < 10 && System.currentTimeMillis() - start < 60000) {
            sleep(100);
            boolean moved = onClient(place);
            int n = onClient(new Callable<Integer>() {
                public Integer call() throws Exception {
                    return ((List) pending.get(mc.renderGlobal)).size();
                }
            });
            quiet = n == 0 && !moved ? quiet + 1 : 0;
        }
        sleep((long) (settle * 1000));
        // Before the shot: the player is still in place and no screen is open.
        // The window pops up in front of whatever the user is doing, so a stray
        // key (Esc) can open one. Fix either and let the scene settle again.
        for (int tries = 0; tries < 5; tries++) {
            boolean fixed = onClient(new Callable<Boolean>() {
                public Boolean call() throws Exception {
                    boolean screen = mc.currentScreen != null;
                    if (screen) {
                        System.out.println("ShotHarness: closing " + mc.currentScreen.getClass().getSimpleName());
                        mc.displayGuiScreen(null);
                    }
                    boolean moved = place.call();
                    if (moved) System.out.println("ShotHarness: player had moved, put back");
                    return screen || moved;
                }
            });
            if (!fixed) break;
            sleep(1500);
        }

        File dir = out.getParentFile();
        final File tmpDir = new File(dir, "vanilla-tmp");
        new File(tmpDir, "screenshots").mkdirs(); // saveScreenshot only creates the last folder
        onClient(new Callable<Object>() {
            public Object call() {
                // the F2 path: reads the framebuffer the last frame was drawn into
                ScreenShotHelper.saveScreenshot(tmpDir, out.getName(), mc.displayWidth, mc.displayHeight, mc.getFramebuffer());
                return null;
            }
        });
        File shot = new File(new File(tmpDir, "screenshots"), out.getName());
        out.delete();
        if (!shot.renameTo(out)) throw new IllegalStateException("screenshot missing: " + shot);
        System.out.println("ShotHarness: saved " + out);
        onClient(new Callable<Object>() {
            public Object call() {
                mc.shutdown();
                return null;
            }
        });
    }
}
