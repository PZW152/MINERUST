import java.io.*;
import java.lang.reflect.Field;
import net.minecraft.block.Block;
import net.minecraft.entity.Entity;
import net.minecraft.init.Bootstrap;
import net.minecraft.world.*;
import net.minecraft.world.chunk.Chunk;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.gen.ChunkProviderGenerate;
import net.minecraft.world.storage.WorldInfo;

public class Harness {
    public static abstract class W extends World {
        W() { super(null, null, (WorldSettings) null, null, null); }
        @Override protected IChunkProvider createChunkProvider() { return null; }
        @Override protected int getRenderDistanceChunks() { return 8; }
        @Override public Entity getEntityByID(int i) { return null; }
        @Override protected boolean chunkExists(int x, int z) { return false; }
    }
    public static class W2 extends W {}

    static void set(Class<?> c, Object o, String name, Object v) throws Exception {
        Field f = c.getDeclaredField(name);
        f.setAccessible(true);
        f.set(o, v);
    }

    public static void main(String[] a) throws Exception {
        long seed = Long.parseLong(a[0]);
        int n = Integer.parseInt(a[1]);
        Bootstrap.register();
        Field uf = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        uf.setAccessible(true);
        sun.misc.Unsafe u = (sun.misc.Unsafe) uf.get(null);
        World w = (World) u.allocateInstance(W2.class);
        Class<?> gt = Class.forName("net.minecraft.world.WorldSettings$GameType");
        Object survival = null;
        for (Object o : gt.getEnumConstants()) if (o.toString().equalsIgnoreCase("SURVIVAL") || ((Enum<?>) o).name().equals("SURVIVAL")) survival = o;
        WorldSettings ws = (WorldSettings) WorldSettings.class.getConstructor(long.class, gt, boolean.class, boolean.class, WorldType.class)
            .newInstance(seed, survival, false, false, WorldType.DEFAULT);
        WorldInfo info = new WorldInfo(ws, "x");
        set(World.class, w, "worldInfo", info);
        set(World.class, w, "worldAccesses", new java.util.ArrayList());
        set(World.class, w, "theProfiler", new net.minecraft.profiler.Profiler());
        WorldProvider p = WorldProvider.getProviderForDimension(0);
        set(World.class, w, "provider", p);
        p.registerWorld(w);
        ChunkProviderGenerate g = new ChunkProviderGenerate(w, seed, false);
        try (DataOutputStream out = new DataOutputStream(new BufferedOutputStream(new FileOutputStream(a[2])))) {
            byte[] blocks = new byte[65536];
            byte[] meta = new byte[65536];
            for (int i = 0; i < n; i++) {
                int cx = ((i * 37 % 101) - 50) * 3;
                int cz = ((i * 53 % 103) - 51) * 3;
                Chunk c = g.provideChunk(cx, cz);
                for (int x = 0; x < 16; x++)
                    for (int z = 0; z < 16; z++)
                        for (int y = 0; y < 256; y++) {
                            int idx = (x * 16 + z) * 256 + y;
                            blocks[idx] = (byte) Block.getIdFromBlock(c.getBlock(x, y, z));
                            meta[idx] = (byte) c.getBlockMetadata(x, y, z);
                        }
                out.write(blocks);
                out.write(meta);
                out.write(c.getBiomeArray());
            }
        }
        System.out.println("done");
    }
}
